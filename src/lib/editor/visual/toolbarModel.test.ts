import { describe, expect, it } from "vitest";
import { t } from "../../i18n";
import { NO_FORMAT } from "./format";
import {
  BLOCK_TYPE_WIDTH,
  MORE_WIDTH,
  SLASH_ITEMS,
  TOOLBAR_ITEMS,
  TOOLBAR_PADDING,
  blockLabel,
  filterSlash,
  visibleCount,
} from "./toolbarModel";

const widths = TOOLBAR_ITEMS.map((i) => i.width);
const total = widths.reduce((a, b) => a + b, 0);
/** The toolbar width that has exactly `room` px for the buttons after the block type. */
const barFor = (room: number): number => room + TOOLBAR_PADDING + BLOCK_TYPE_WIDTH;

describe("visibleCount", () => {
  it("shows every button when they all fit, and no `[...]`", () => {
    expect(visibleCount(widths, barFor(total))).toBe(widths.length);
    expect(visibleCount(widths, 4000)).toBe(widths.length);
  });

  it("folds the tail into the menu when they do not, leaving room for the `[...]` button", () => {
    const count = visibleCount(widths, barFor(total - 1));
    expect(count).toBeLessThan(widths.length);
    const used = widths.slice(0, count).reduce((a, b) => a + b, 0);
    expect(used + MORE_WIDTH).toBeLessThanOrEqual(total - 1);
    // and one more button would not have fitted
    expect(used + widths[count] + MORE_WIDTH).toBeGreaterThan(total - 1);
  });

  it("shows fewer buttons as the toolbar narrows, down to none", () => {
    let last = widths.length;
    for (let bar = barFor(total); bar >= 0; bar -= 10) {
      const count = visibleCount(widths, bar);
      expect(count).toBeLessThanOrEqual(last);
      last = count;
    }
    expect(visibleCount(widths, 100)).toBe(0);
    expect(visibleCount([], 100)).toBe(0);
  });
});

describe("the buttons", () => {
  it("have one id each, a label that exists and a shortcut where the design gives one", () => {
    const ids = TOOLBAR_ITEMS.map((i) => i.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const item of TOOLBAR_ITEMS) expect(t(item.label)).not.toBe("");
    const hint = Object.fromEntries(TOOLBAR_ITEMS.map((i) => [i.id, i.hint]));
    expect(hint).toMatchObject({
      bold: "Ctrl+B",
      italic: "Ctrl+I",
      strike: "Ctrl+Shift+X",
      code: "Ctrl+`",
      link: "Ctrl+K",
      codeBlock: "Ctrl+Alt+C",
    });
  });

  it("show a pressed state from the selection", () => {
    const pressed = (patch: Partial<typeof NO_FORMAT>): string[] =>
      TOOLBAR_ITEMS.filter((i) => i.active({ ...NO_FORMAT, ...patch })).map((i) => i.id);
    expect(pressed({})).toEqual([]);
    expect(pressed({ bold: true, italic: true })).toEqual(["bold", "italic"]);
    expect(pressed({ list: "task" })).toEqual(["task"]);
    expect(pressed({ quote: true, table: true })).toEqual(["quote", "table"]);
    expect(pressed({ block: "code" })).toEqual(["codeBlock"]);
  });
});

describe("blockLabel", () => {
  it("names the block the caret is in", () => {
    expect(t(blockLabel("paragraph"))).toBe("Paragraph");
    expect(t(blockLabel("h2"))).toBe("Heading 2");
    expect(t(blockLabel("h6"))).toBe("Heading 6");
    expect(t(blockLabel("code"))).toBe("Code block");
    expect(t(blockLabel("other"))).toBe("Paragraph");
  });
});

describe("filterSlash", () => {
  const names = (query: string): string[] => filterSlash(SLASH_ITEMS, query, t).map((i) => i.id);

  it("lists everything for an empty query, in order", () => {
    expect(names("")).toHaveLength(SLASH_ITEMS.length);
    expect(names("")[0]).toBe("paragraph");
  });

  it("matches the label and the extra words, case-insensitively, every word of the query", () => {
    expect(names("quo")).toEqual(["quote"]);
    expect(names("HEAD")).toEqual(["h1", "h2", "h3", "h4"]);
    expect(names("todo")).toEqual(["task"]);
    expect(names("list num")).toEqual(["ordered"]);
    expect(names("divider")).toEqual(["rule"]);
  });

  it("returns nothing when no item matches", () => {
    expect(names("zzz")).toEqual([]);
  });
});
