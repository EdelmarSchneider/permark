import { describe, expect, it } from "vitest";
import type { MenuEntry } from "../../components/Menu.svelte";
import { t } from "../../i18n";
import { NO_FORMAT, tableUnavailable } from "./format";
import {
  BLOCK_TYPES,
  BLOCK_TYPE_WIDTH,
  MORE_WIDTH,
  SLASH_ITEMS,
  TOOLBAR_ITEMS,
  TOOLBAR_PADDING,
  blockLabel,
  filterSlash,
  moreMenuEntries,
  tableMenuEntries,
  visibleCount,
  type ToolbarItem,
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

describe("block types", () => {
  it("shows the chord that sets the paragraph and headings 1 to 6", () => {
    expect(BLOCK_TYPES.map((b) => [b.id, b.hint])).toEqual([
      ["paragraph", "Ctrl+Shift+0"],
      ["h1", "Ctrl+Shift+1"],
      ["h2", "Ctrl+Shift+2"],
      ["h3", "Ctrl+Shift+3"],
      ["h4", "Ctrl+Shift+4"],
      ["h5", "Ctrl+Shift+5"],
      ["h6", "Ctrl+Shift+6"],
    ]);
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

type Item = Extract<MenuEntry, { onselect: () => void }>;
const itemsOf = (entries: MenuEntry[]): Item[] =>
  entries.filter(
    (e): e is Item => e.type !== "separator" && e.type !== "label" && e.type !== "note",
  );
const byId = (entries: MenuEntry[], id: string): Item => {
  const found = itemsOf(entries).find((e) => e.id === id);
  if (!found) throw new Error(`no entry ${id}`);
  return found;
};
const labelsOf = (entries: MenuEntry[]): string[] =>
  entries.flatMap((e) => (e.type === "label" ? [e.label] : []));
const notesOf = (entries: MenuEntry[]): string[] =>
  entries.flatMap((e) => (e.type === "note" ? [e.text] : []));
const TABLE_ACTIONS = [
  "rowBefore",
  "rowAfter",
  "colBefore",
  "colAfter",
  "deleteRow",
  "deleteCol",
  "deleteTable",
] as const;

describe("tableUnavailable", () => {
  it("lets every table command run in a body row", () => {
    for (const id of TABLE_ACTIONS) {
      expect(tableUnavailable({ ...NO_FORMAT, table: true }, id), id).toBeNull();
    }
  });

  it("refuses every table command outside a table", () => {
    for (const id of TABLE_ACTIONS) expect(tableUnavailable(NO_FORMAT, id), id).toBe("notInTable");
  });

  it("keeps a row above and the deletion of a row out of the header row, and nothing else", () => {
    const header = { ...NO_FORMAT, table: true, tableHeader: true };
    expect(tableUnavailable(header, "rowBefore")).toBe("headerRowAbove");
    expect(tableUnavailable(header, "deleteRow")).toBe("headerRowDelete");
    // a row below the header is the first body row of the table
    for (const id of ["rowAfter", "colBefore", "colAfter", "deleteCol", "deleteTable"] as const) {
      expect(tableUnavailable(header, id), id).toBeNull();
    }
  });
});

describe("the table menu", () => {
  const picked: string[] = [];
  const menu = (patch: Partial<typeof NO_FORMAT>): MenuEntry[] =>
    tableMenuEntries({ ...NO_FORMAT, ...patch }, t, (id) => void picked.push(id));

  it("inserts a table and edits the rows and columns of the one the caret is in", () => {
    const entries = menu({ table: true });
    expect(itemsOf(entries).map((e) => e.id)).toEqual(["insert", ...TABLE_ACTIONS]);
    for (const entry of itemsOf(entries)) expect(entry.disabled, entry.id).toBeFalsy();
    picked.length = 0;
    byId(entries, "rowAfter").onselect();
    byId(entries, "deleteRow").onselect();
    byId(entries, "insert").onselect();
    expect(picked).toEqual(["rowAfter", "deleteRow", "table"]);
  });

  it("says, on every entry that is off, that the caret has to be in a table cell", () => {
    const entries = menu({});
    expect(byId(entries, "insert").disabled).toBeFalsy();
    for (const id of TABLE_ACTIONS) {
      expect(byId(entries, id).disabled, id).toBe(true);
      expect(byId(entries, id).title, id).toBe(t("toolbar.tableNeedsCell"));
    }
    // a line that shows without hovering: a keyboard has no tooltips
    expect(notesOf(entries)).toEqual([t("toolbar.tableNeedsCell")]);
    expect(notesOf(menu({ table: true }))).toEqual([]);
  });

  it("starts the keyboard on a row action, not on 'Insert table', when the caret is in a table", () => {
    const initial = (patch: Partial<typeof NO_FORMAT>): string[] =>
      itemsOf(menu(patch))
        .filter((e) => e.initial)
        .map((e) => e.id);
    expect(initial({ table: true })).toEqual(["rowBefore"]);
    expect(initial({ table: true, tableHeader: true })).toEqual(["rowAfter"]);
    expect(initial({})).toEqual([]);
  });

  it("explains the header row: no row above it, no deleting it, a row below it is the first row of the table", () => {
    const entries = menu({ table: true, tableHeader: true });
    expect(byId(entries, "rowBefore").disabled).toBe(true);
    expect(byId(entries, "rowBefore").title).toBe(t("toolbar.tableHeaderNoRowAbove"));
    expect(byId(entries, "deleteRow").disabled).toBe(true);
    expect(byId(entries, "deleteRow").title).toBe(t("toolbar.tableHeaderNoDelete"));
    for (const id of ["insert", "rowAfter", "colBefore", "colAfter", "deleteCol", "deleteTable"]) {
      expect(byId(entries, id).disabled, id).toBeFalsy();
      expect(byId(entries, id).title, id).toBeUndefined();
    }
  });
});

describe("the menu of the folded buttons", () => {
  const act: string[] = [];
  const ran: string[] = [];
  const foldedFrom = (id: string): ToolbarItem[] =>
    TOOLBAR_ITEMS.slice(TOOLBAR_ITEMS.findIndex((i) => i.id === id));
  const more = (folded: readonly ToolbarItem[], patch: Partial<typeof NO_FORMAT>, off = false) =>
    moreMenuEntries(
      folded,
      { ...NO_FORMAT, ...patch },
      off,
      t,
      (item) => void act.push(item.id),
      (id) => void ran.push(id),
    );

  it("keeps the rows and columns of a table when the table button is folded away (it was only 'insert a table')", () => {
    const entries = more(foldedFrom("table"), { table: true });
    expect(itemsOf(entries).map((e) => e.id)).toEqual([
      "insert",
      ...TABLE_ACTIONS,
      "codeBlock",
      "image",
      "rule",
    ]);
    expect(byId(entries, "rowAfter").disabled).toBeFalsy();
    expect(byId(entries, "deleteRow").disabled).toBeFalsy();
    expect(labelsOf(entries)).toContain(t("toolbar.table"));
    ran.length = 0;
    act.length = 0;
    byId(entries, "rowAfter").onselect();
    byId(entries, "insert").onselect();
    byId(entries, "codeBlock").onselect();
    expect(ran).toEqual(["rowAfter", "table"]);
    expect(act).toEqual(["codeBlock"]);
  });

  it("explains the entries that are off, as the table menu does", () => {
    const entries = more(foldedFrom("table"), { table: true, tableHeader: true });
    expect(byId(entries, "rowBefore").title).toBe(t("toolbar.tableHeaderNoRowAbove"));
    expect(byId(more(foldedFrom("table"), {}), "colAfter").title).toBe(t("toolbar.tableNeedsCell"));
  });

  it("has no table entries when the table button is still in the toolbar", () => {
    const entries = more(foldedFrom("codeBlock"), { table: true });
    expect(itemsOf(entries).map((e) => e.id)).toEqual(["codeBlock", "image", "rule"]);
  });

  it("marks the format that is on, and turns everything off on a locked block", () => {
    const quote = more(foldedFrom("quote"), { quote: true });
    expect(byId(quote, "quote").checked).toBe(true);
    const atom = more(foldedFrom("table"), { table: true, atom: true }, true);
    for (const entry of itemsOf(atom)) expect(entry.disabled, entry.id).toBe(true);
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
