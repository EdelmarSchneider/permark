import { describe, expect, it } from "vitest";
import {
  CollapseMemory,
  MAX_HEADINGS,
  buildOutline,
  currentHeading,
  navigate,
  visibleAncestor,
  visibleRows,
  type BlockInfo,
} from "./outline";

const h = (level: number, text: string, changed = false): BlockInfo => ({ level, text, changed });
const block = (changed = false): BlockInfo => ({ level: null, text: "", changed });

/** h1 A, h2 B, h3 C, h2 D, h1 E */
const sample = (): BlockInfo[] => [
  h(1, "A"),
  block(),
  h(2, "B"),
  h(3, "C"),
  block(),
  h(2, "D"),
  h(1, "E"),
];

describe("buildOutline: extracting headings and building the tree", () => {
  it("keeps the headings in document order and ignores the other blocks", () => {
    const { items, total } = buildOutline(sample());
    expect(items.map((i) => i.text)).toEqual(["A", "B", "C", "D", "E"]);
    expect(items.map((i) => i.level)).toEqual([1, 2, 3, 2, 1]);
    expect(total).toBe(5);
  });

  it("nests each heading under the nearest heading above it with a lower level", () => {
    const { items } = buildOutline(sample());
    expect(items.map((i) => i.parent)).toEqual([null, 0, 1, 0, null]);
    expect(items.map((i) => i.depth)).toEqual([0, 1, 2, 1, 0]);
    expect(items.map((i) => i.hasChildren)).toEqual([true, true, false, false, false]);
    // `end` is where the subtree stops
    expect(items.map((i) => i.end)).toEqual([4, 3, 3, 4, 5]);
  });

  it("indents by nesting, so a skipped level adds no gap", () => {
    const { items } = buildOutline([h(2, "Top"), h(4, "Deep"), h(3, "Mid")]);
    expect(items.map((i) => i.depth)).toEqual([0, 1, 1]);
    expect(items.map((i) => i.parent)).toEqual([null, 0, 0]);
  });

  it("starts at any level: a document with no h1 is not indented", () => {
    const { items } = buildOutline([h(3, "x"), h(3, "y")]);
    expect(items.map((i) => i.depth)).toEqual([0, 0]);
  });

  it("is empty when there are no headings", () => {
    expect(buildOutline([block(), block()])).toEqual({ items: [], total: 0 });
    expect(buildOutline([])).toEqual({ items: [], total: 0 });
  });

  it("numbers headings with the same text so each has its own collapse key", () => {
    const { items } = buildOutline([h(2, "Notes"), h(2, "Intro"), h(2, "Notes"), h(3, "Notes")]);
    const keys = items.map((i) => i.key);
    expect(new Set(keys).size).toBe(4);
    expect(keys[1]).toBe("Intro");
    // the first one keeps the plain text, so it stays stable when a duplicate is added later
    expect(keys[0]).toBe("Notes");
  });
});

describe("buildOutline: sections with live changes", () => {
  it("marks the section that holds a changed block", () => {
    const { items } = buildOutline([
      h(1, "A"),
      block(),
      h(2, "B"),
      block(true),
      h(2, "C"),
      block(),
    ]);
    expect(items.map((i) => i.changed)).toEqual([false, true, false]);
  });

  it("marks a section whose own heading changed", () => {
    const { items } = buildOutline([h(1, "A"), h(2, "B", true)]);
    expect(items.map((i) => i.changed)).toEqual([false, true]);
  });

  it("puts a changed block under the last heading, not the one it is nested in", () => {
    const { items } = buildOutline([h(1, "A"), h(2, "B"), h(1, "C"), block(true)]);
    expect(items.map((i) => i.changed)).toEqual([false, false, true]);
  });

  it("ignores changes above the first heading", () => {
    const { items } = buildOutline([block(true), h(1, "A")]);
    expect(items.map((i) => i.changed)).toEqual([false]);
  });

  it("tells a parent that something below it changed", () => {
    const { items } = buildOutline([h(1, "A"), h(2, "B"), h(3, "C", true), h(2, "D"), h(1, "E")]);
    expect(items.map((i) => i.subtreeChanged)).toEqual([true, true, false, false, false]);
    expect(items.map((i) => i.changed)).toEqual([false, false, true, false, false]);
  });

  it("has no marks when nothing changed", () => {
    const { items } = buildOutline(sample());
    expect(items.every((i) => !i.changed && !i.subtreeChanged)).toBe(true);
  });
});

describe("buildOutline: very long documents", () => {
  const many = (n: number): BlockInfo[] =>
    Array.from({ length: n }, (_, i) => h(2, `Heading ${i + 1}`));

  it("keeps the first 1,000 headings and reports how many there are", () => {
    const { items, total } = buildOutline(many(1312));
    expect(MAX_HEADINGS).toBe(1000);
    expect(items).toHaveLength(1000);
    expect(items[999].text).toBe("Heading 1000");
    expect(total).toBe(1312);
  });

  it("shows everything at exactly 1,000", () => {
    const { items, total } = buildOutline(many(1000));
    expect(items).toHaveLength(1000);
    expect(total).toBe(1000);
  });

  it("does not hand the changes below the cut to the last heading it kept", () => {
    const blocks = [...many(1000), h(2, "Cut off"), block(true)];
    const { items } = buildOutline(blocks);
    expect(items[999].changed).toBe(false);
  });
});

describe("visibleRows: collapsing", () => {
  const { items } = buildOutline(sample());

  it("lists every heading when nothing is collapsed", () => {
    expect(visibleRows(items, new Set())).toEqual([0, 1, 2, 3, 4]);
  });

  it("hides the whole subtree of a collapsed heading", () => {
    expect(visibleRows(items, new Set(["A"]))).toEqual([0, 4]);
    expect(visibleRows(items, new Set(["B"]))).toEqual([0, 1, 3, 4]);
  });

  it("ignores a collapse key on a heading that has nothing below it", () => {
    expect(visibleRows(items, new Set(["C"]))).toEqual([0, 1, 2, 3, 4]);
  });

  it("keeps an inner collapse while the outer one is open again", () => {
    expect(visibleRows(items, new Set(["B", "E"]))).toEqual([0, 1, 3, 4]);
  });
});

describe("CollapseMemory: collapse state per document, by heading text", () => {
  it("is empty for a document nobody collapsed anything in", () => {
    expect(new CollapseMemory().get("a.md").size).toBe(0);
  });

  it("keeps what was collapsed when the document is rebuilt, wherever the headings moved", () => {
    const memory = new CollapseMemory();
    memory.set("a.md", "A", true);
    memory.set("a.md", "B", true);
    // the agent added a section at the top and reworded a paragraph; A and B are still there
    const rebuilt = buildOutline([h(1, "New"), h(2, "Fresh"), ...sample()]);
    const rows = visibleRows(rebuilt.items, memory.get("a.md"));
    expect(rows.map((i) => rebuilt.items[i].text)).toEqual(["New", "Fresh", "A", "E"]);
  });

  it("forgets the collapse of a heading that was renamed", () => {
    const memory = new CollapseMemory();
    memory.set("a.md", "A", true);
    const rebuilt = buildOutline([h(1, "A renamed"), h(2, "B")]);
    expect(visibleRows(rebuilt.items, memory.get("a.md"))).toEqual([0, 1]);
  });

  it("keeps each document apart", () => {
    const memory = new CollapseMemory();
    memory.set("a.md", "A", true);
    expect([...memory.get("a.md")]).toEqual(["A"]);
    expect(memory.get("b.md").size).toBe(0);
  });

  it("expands again", () => {
    const memory = new CollapseMemory();
    memory.set("a.md", "A", true);
    memory.set("a.md", "A", false);
    expect(memory.get("a.md").size).toBe(0);
  });

  it("hands out a new set on every change, so the UI sees it", () => {
    const memory = new CollapseMemory();
    const before = memory.get("a.md");
    const after = memory.set("a.md", "A", true);
    expect(after).not.toBe(before);
    expect(memory.get("a.md")).toBe(after);
  });

  it("remembers a limited number of documents, dropping the one not used for longest", () => {
    const memory = new CollapseMemory(2);
    memory.set("a.md", "A", true);
    memory.set("b.md", "B", true);
    memory.set("a.md", "A2", true); // a.md is the most recent again
    memory.set("c.md", "C", true);
    expect(memory.get("b.md").size).toBe(0);
    expect(memory.get("a.md").size).toBe(2);
    expect(memory.get("c.md").size).toBe(1);
  });

  it("forgets everything on clear", () => {
    const memory = new CollapseMemory();
    memory.set("a.md", "A", true);
    memory.clear();
    expect(memory.get("a.md").size).toBe(0);
  });
});

describe("visibleAncestor: which row stands for a hidden heading", () => {
  const { items } = buildOutline(sample());

  it("is the heading itself when it is visible", () => {
    expect(visibleAncestor(items, new Set(), 2)).toBe(2);
    expect(visibleAncestor(items, new Set(["C"]), 2)).toBe(2);
  });

  it("is the collapsed parent when the heading is hidden inside it", () => {
    expect(visibleAncestor(items, new Set(["B"]), 2)).toBe(1);
  });

  it("is the outermost collapsed ancestor", () => {
    expect(visibleAncestor(items, new Set(["A", "B"]), 2)).toBe(0);
    expect(visibleAncestor(items, new Set(["A"]), 3)).toBe(0);
  });

  it("is nothing for no heading", () => {
    expect(visibleAncestor(items, new Set(), null)).toBeNull();
  });
});

describe("currentHeading: the section being read", () => {
  // heading tops, in px from the top of the scroller
  const tops = [-400, -120, 90, 300, 900];
  const topOf = (i: number): number => tops[i];

  it("is the last heading above the line", () => {
    expect(currentHeading(5, topOf, { line: 200 })).toBe(2);
    expect(currentHeading(5, topOf, { line: 300 })).toBe(3);
    expect(currentHeading(5, topOf, { line: 1000 })).toBe(4);
  });

  it("is nothing above the first heading", () => {
    expect(currentHeading(5, (i) => tops[i] + 1000, { line: 200 })).toBeNull();
    expect(currentHeading(0, topOf, { line: 200 })).toBeNull();
  });

  it("stays on the heading until the next one crosses the line", () => {
    expect(currentHeading(5, topOf, { line: 299 })).toBe(2);
    expect(currentHeading(5, topOf, { line: 301 })).toBe(3);
  });

  it("finds the heading with a binary search: few measurements for a long list", () => {
    const n = 1000;
    let calls = 0;
    const result = currentHeading(
      n,
      (i) => {
        calls++;
        return i * 100 - 40_000;
      },
      { line: 0 },
    );
    expect(result).toBe(400);
    expect(calls).toBeLessThan(25);
  });

  it("at the bottom of the page, the last heading on screen counts even if it is below the line", () => {
    // 240px viewport, line at 80: the last heading sits at 150, in view but under the line
    const bottom = [-300, 20, 150];
    expect(currentHeading(3, (i) => bottom[i], { line: 80 })).toBe(1);
    expect(currentHeading(3, (i) => bottom[i], { line: 80, bottomEdge: 240 })).toBe(2);
  });

  it("at the bottom, a heading out of view still does not count", () => {
    const bottom = [-300, 20, 400];
    expect(currentHeading(3, (i) => bottom[i], { line: 80, bottomEdge: 240 })).toBe(1);
  });
});

describe("navigate: the keyboard in the list", () => {
  const { items } = buildOutline(sample());
  const all = (collapsed: string[] = []) => {
    const set = new Set(collapsed);
    return { rows: visibleRows(items, set), set };
  };

  it("moves between the visible rows", () => {
    const { rows, set } = all(["B"]);
    expect(navigate(items, rows, 0, set, "ArrowDown")).toEqual({ cursor: 1 });
    expect(navigate(items, rows, 1, set, "ArrowDown")).toEqual({ cursor: 3 });
    expect(navigate(items, rows, 3, set, "ArrowUp")).toEqual({ cursor: 1 });
    expect(navigate(items, rows, 0, set, "ArrowUp")).toEqual({ cursor: 0 });
    expect(navigate(items, rows, 4, set, "ArrowDown")).toEqual({ cursor: 4 });
    expect(navigate(items, rows, 3, set, "Home")).toEqual({ cursor: 0 });
    expect(navigate(items, rows, 0, set, "End")).toEqual({ cursor: 4 });
  });

  it("opens a heading with Enter or Space", () => {
    const { rows, set } = all();
    expect(navigate(items, rows, 2, set, "Enter")).toEqual({ cursor: 2, open: 2 });
    expect(navigate(items, rows, 3, set, " ")).toEqual({ cursor: 3, open: 3 });
  });

  it("Left collapses an open heading, then moves to its parent", () => {
    const { rows, set } = all();
    expect(navigate(items, rows, 1, set, "ArrowLeft")).toEqual({
      cursor: 1,
      toggle: { index: 1, expand: false },
    });
    // a leaf goes to its parent
    expect(navigate(items, rows, 2, set, "ArrowLeft")).toEqual({ cursor: 1 });
    // a top-level heading with nothing to collapse stays put
    expect(navigate(items, rows, 4, set, "ArrowLeft")).toEqual({ cursor: 4 });
  });

  it("Left on an already collapsed heading goes to its parent", () => {
    const { rows, set } = all(["B"]);
    expect(navigate(items, rows, 1, set, "ArrowLeft")).toEqual({ cursor: 0 });
  });

  it("Right expands a collapsed heading, then moves to its first child", () => {
    const collapsed = all(["B"]);
    expect(navigate(items, collapsed.rows, 1, collapsed.set, "ArrowRight")).toEqual({
      cursor: 1,
      toggle: { index: 1, expand: true },
    });
    const open = all();
    expect(navigate(items, open.rows, 1, open.set, "ArrowRight")).toEqual({ cursor: 2 });
    // a leaf stays put
    expect(navigate(items, open.rows, 2, open.set, "ArrowRight")).toEqual({ cursor: 2 });
  });

  it("starts from the row that stands for a cursor hidden by a collapse", () => {
    const { rows, set } = all(["B"]);
    // the cursor was on C, which is now hidden inside B
    expect(navigate(items, rows, 2, set, "ArrowDown")).toEqual({ cursor: 3 });
  });

  it("leaves other keys alone, and does nothing without headings", () => {
    const { rows, set } = all();
    expect(navigate(items, rows, 0, set, "a")).toBeNull();
    expect(navigate(items, rows, 0, set, "Tab")).toBeNull();
    expect(navigate([], [], 0, new Set(), "ArrowDown")).toBeNull();
  });
});
