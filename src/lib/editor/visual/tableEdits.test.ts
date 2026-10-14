// @vitest-environment jsdom
// The table menu's commands and the table keys on the document as it is saved: adding and deleting rows and columns rewrites the
// table block and nothing else. The first part is a small table that ends the file (with and without a final line break, with CRLF
// and a byte order mark); the second is the keys of a table (Enter, Shift+Enter, Ctrl+Enter, Tab) cell by cell; the third runs every
// command on every table of the corpus.

import { undo } from "@milkdown/kit/prose/history";
import { Selection } from "@milkdown/kit/prose/state";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { mockParseBlocks } from "../../mock/mockBlocks";
import { buildBlockMap, type BlockMap } from "../blockModel";
import { splitters } from "../corpusDocs";
import { startOf } from "./docPositions";
import "./jsdomShims";
import { VisualEditor } from "./editor";
import type { CommandId } from "./format";
import { tableKeysKey } from "./plugins";

let editor: VisualEditor;

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.appendChild(host);
  editor = await VisualEditor.create({
    host,
    label: "tables",
    scroll: false,
    imageUrl: (src) => src,
    locked: {
      render: (body, atom) => {
        body.textContent = atom.source;
      },
      edit: () => {},
      link: () => {},
      labels: { edit: "Edit in Source", editHint: "", region: (kind) => kind, reason: () => "" },
    },
    handlers: {
      changed: () => {},
      files: () => {},
      editInSource: () => {},
      link: () => {},
      slash: () => {},
      slashKey: () => false,
    },
  });
});
afterAll(async () => {
  await editor.destroy();
});

/** Where each block starts in the text. */
function blockStarts(map: BlockMap): number[] {
  let at = map.seps[0].length;
  return map.blocks.map((block, i) => {
    const start = at;
    at += block.text.length + map.seps[i + 1].length;
    return start;
  });
}

/** Puts the caret in the first cell of the header row or of the first body row of the table that is node `index`. */
function caretIn(index: number, row: "header" | "body"): boolean {
  const { view } = editor.core;
  const { doc } = view.state;
  const start = startOf(doc, index);
  let at: number | null = null;
  doc.child(index).forEach((node, offset) => {
    if (at !== null) return;
    if ((node.type.name === "table_header_row") === (row === "header")) {
      // the row, its first cell, and the paragraph inside it
      at = start + 1 + offset + 2;
    }
  });
  if (at === null) return false;
  view.dispatch(view.state.tr.setSelection(Selection.near(doc.resolve(at), 1)));
  return editor.format().table;
}

/** Puts the caret at the end of the text of the last cell of the table that is node `index`. */
function caretInLastCell(index: number): boolean {
  const { view } = editor.core;
  const { doc } = view.state;
  // the table's end, then the closing tokens of the row, the cell and the paragraph
  const end = startOf(doc, index) + doc.child(index).nodeSize - 4;
  view.dispatch(view.state.tr.setSelection(Selection.near(doc.resolve(end), -1)));
  return editor.format().table;
}

/** A key pressed in the editor: whether a plugin took it (false: the browser would do what it does with the key). */
function press(key: string, init: KeyboardEventInit = {}): boolean {
  const { view } = editor.core;
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init });
  return view.someProp("handleKeyDown", (handler) => handler(view, event)) === true;
}

/** A key pressed in the editor, offered to the table keys alone (the plugin, not the presets' keys after it). */
function pressTableKeys(key: string, init: KeyboardEventInit = {}): boolean {
  const { view } = editor.core;
  const plugin = tableKeysKey.get(view.state);
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init });
  return plugin?.props.handleKeyDown?.call(plugin, view, event) === true;
}

/** The row the caret is in, counted from the top of its table (0 is the header row). */
function caretRow(): number {
  const { $from } = editor.core.view.state.selection;
  for (let depth = $from.depth; depth > 0; depth--) {
    if ($from.node(depth).type.name === "table") return $from.index(depth);
  }
  return -1;
}

/** The cell the caret is in (row 0 is the header row), or null when the caret is not in a table. */
function caretCell(): { row: number; col: number } | null {
  const { $head } = editor.core.view.state.selection;
  for (let depth = $head.depth; depth > 0; depth--) {
    if ($head.node(depth).type.name === "table") {
      return { row: $head.index(depth), col: $head.index(depth + 1) };
    }
  }
  return null;
}

/** Puts the caret at the end of the text of one cell of the table that is node `index`. */
function caretInCell(index: number, row: number, col: number): void {
  const { view } = editor.core;
  const { doc } = view.state;
  const table = doc.child(index);
  let at = startOf(doc, index) + 1;
  for (let r = 0; r < row; r++) at += table.child(r).nodeSize;
  const tableRow = table.child(row);
  at += 1;
  for (let c = 0; c < col; c++) at += tableRow.child(c).nodeSize;
  // the end of the cell's content: the closing token of the cell is behind it
  at += tableRow.child(col).nodeSize - 1;
  view.dispatch(view.state.tr.setSelection(Selection.near(doc.resolve(at), -1)));
}

const rowsOf = (index: number): number => editor.core.view.state.doc.child(index).childCount;
const columnsOf = (index: number): number =>
  editor.core.view.state.doc.child(index).firstChild?.childCount ?? 0;

/** What a table that ends a file looks like to the toolbar's commands (invented content). */
const INTRO = "Some intro paragraph.";
const TABLE = [
  "| Place | What it was |",
  "|---|---|",
  "| Mercer Hill | a house on a hill |",
  "| Ashgrove | a village with an old church |",
].join("\n");

const BOM = String.fromCharCode(0xfeff);
const variants = [
  ["a table that ends the file without a line break", `${INTRO}\n\n${TABLE}`],
  ["a table that ends the file with a line break", `${INTRO}\n\n${TABLE}\n`],
  ["a table that ends the file, with CRLF", `${INTRO}\n\n${TABLE}`.replace(/\n/g, "\r\n")],
  [
    "a table that ends the file, with CRLF and a byte order mark",
    BOM + `${INTRO}\n\n${TABLE}\r\n`.replace(/\n/g, "\r\n"),
  ],
  ["a table that is the whole file", TABLE],
] as const;

describe.each(variants)("%s", (_name, text) => {
  const map = buildBlockMap(text, mockParseBlocks(text));
  const index = map.blocks.findIndex((b) => b.kind === "table");
  const start = blockStarts(map)[index];
  const end = start + map.blocks[index].text.length;
  const eol = text.includes("\r\n") ? "\r\n" : "\n";

  const load = (): void => {
    const report = editor.load({ map });
    expect(report.reasons.has(index), "the table is editable").toBe(false);
  };
  /** The saved table block, with the bytes around it checked to be what they were. */
  const savedTable = (): string[] => {
    const after = editor.text();
    expect(after.slice(0, start), "the bytes before the table").toBe(text.slice(0, start));
    expect(after.endsWith(text.slice(end)), "the bytes after the table").toBe(true);
    expect(editor.status()).toMatchObject({ dirty: true, changed: 1 });
    return after.slice(start, after.length - (text.length - end)).split(eol);
  };
  const isEmptyRow = (line: string): boolean => /^\|(\s+\|)+$/.test(line);

  it("is one editable block, saved back as the same bytes", () => {
    load();
    expect(editor.text()).toBe(text);
    expect(editor.status()).toEqual({ dirty: false, changed: 0 });
  });

  it("gets its first body row from 'Add row below' in the header row", () => {
    load();
    expect(caretIn(index, "header")).toBe(true);
    expect(editor.format().tableHeader).toBe(true);
    expect(editor.run("rowAfter")).toBe(true);
    const lines = savedTable();
    expect(lines).toHaveLength(5);
    expect(lines[0]).toMatch(/^\| Place\s+\| What it was\s+\|$/);
    expect(isEmptyRow(lines[2])).toBe(true);
    expect(lines[3]).toContain("Mercer Hill");
    expect(lines[4]).toContain("Ashgrove");
  });

  it("refuses a row above the header and the deletion of the header row, and changes nothing", () => {
    load();
    expect(caretIn(index, "header")).toBe(true);
    expect(editor.run("rowBefore")).toBe(false);
    expect(editor.run("deleteRow")).toBe(false);
    expect(editor.text()).toBe(text);
    expect(editor.status()).toEqual({ dirty: false, changed: 0 });
  });

  it("adds a row below the first body row", () => {
    load();
    expect(caretIn(index, "body")).toBe(true);
    expect(editor.run("rowAfter")).toBe(true);
    const lines = savedTable();
    expect(lines).toHaveLength(5);
    expect(lines[2]).toContain("Mercer Hill");
    expect(isEmptyRow(lines[3])).toBe(true);
    expect(lines[4]).toContain("Ashgrove");
  });

  it("adds a row below the last row, which ends the table and the file", () => {
    load();
    expect(caretInLastCell(index)).toBe(true);
    expect(editor.run("rowAfter")).toBe(true);
    const lines = savedTable();
    expect(lines).toHaveLength(5);
    expect(lines[3]).toContain("Ashgrove");
    expect(isEmptyRow(lines[4])).toBe(true);
  });

  it("deletes the last row", () => {
    load();
    expect(caretInLastCell(index)).toBe(true);
    expect(editor.run("deleteRow")).toBe(true);
    const lines = savedTable();
    expect(lines).toHaveLength(3);
    expect(lines[2]).toContain("Mercer Hill");
  });

  it("gets a row from Tab in its last cell, and the caret goes to the first cell of that row", () => {
    load();
    expect(caretInLastCell(index)).toBe(true);
    expect(press("Tab")).toBe(true);
    expect(rowsOf(index)).toBe(4);
    expect(caretRow()).toBe(3);
    const lines = savedTable();
    expect(lines).toHaveLength(5);
    expect(isEmptyRow(lines[4])).toBe(true);
    // the caret is in the new row's first cell: typing goes there
    const { view } = editor.core;
    view.dispatch(view.state.tr.insertText("Elm Row"));
    expect(savedTable()[4]).toMatch(/^\| Elm Row\s+\|/);
  });

  it("moves to the next cell with Tab anywhere else in the table, and leaves Tab alone outside it", () => {
    load();
    expect(caretIn(index, "header")).toBe(true);
    expect(press("Tab")).toBe(true);
    expect(rowsOf(index)).toBe(3);
    expect(editor.text()).toBe(text);
    // not in a table: Tab is not taken, so the browser keeps its meaning for it (moving on to the next control)
    const { view } = editor.core;
    view.dispatch(view.state.tr.setSelection(Selection.atStart(view.state.doc)));
    if (index > 0) {
      expect(editor.format().table).toBe(false);
      expect(press("Tab")).toBe(false);
    }
  });

  it("gets a row from Enter in the last row, in the column the caret was in, and changes only the table block", () => {
    load();
    expect(caretInLastCell(index)).toBe(true);
    expect(press("Enter")).toBe(true);
    expect(rowsOf(index)).toBe(4);
    expect(caretCell()).toEqual({ row: 3, col: 1 });
    const lines = savedTable();
    expect(lines).toHaveLength(5);
    expect(lines[3]).toContain("Ashgrove");
    expect(isEmptyRow(lines[4])).toBe(true);
    // the caret is in the new row's second cell: typing goes there
    const { view } = editor.core;
    view.dispatch(view.state.tr.insertText("Elm Row"));
    expect(savedTable()[4]).toMatch(/^\|\s+\| Elm Row\s+\|$/);
  });

  it("moves down with Enter from the header row and a body row and leaves every byte as it was", () => {
    load();
    expect(caretIn(index, "header")).toBe(true);
    expect(press("Enter")).toBe(true);
    expect(caretCell()).toEqual({ row: 1, col: 0 });
    expect(press("Enter")).toBe(true);
    expect(caretCell()).toEqual({ row: 2, col: 0 });
    expect(rowsOf(index)).toBe(3);
    expect(editor.text()).toBe(text);
    expect(editor.status()).toEqual({ dirty: false, changed: 0 });
  });

  it("adds a row above a body row", () => {
    load();
    expect(caretIn(index, "body")).toBe(true);
    expect(editor.run("rowBefore")).toBe(true);
    const lines = savedTable();
    expect(lines).toHaveLength(5);
    expect(isEmptyRow(lines[2])).toBe(true);
    expect(lines[3]).toContain("Mercer Hill");
  });

  it("deletes a body row", () => {
    load();
    expect(caretIn(index, "body")).toBe(true);
    expect(editor.run("deleteRow")).toBe(true);
    const lines = savedTable();
    expect(lines).toHaveLength(3);
    expect(lines.join(eol)).not.toContain("Mercer Hill");
    expect(lines[2]).toContain("Ashgrove");
  });

  it("adds and deletes a column", () => {
    load();
    expect(caretIn(index, "body")).toBe(true);
    expect(editor.run("colAfter")).toBe(true);
    expect(savedTable()[0].split("|")).toHaveLength(5);
    expect(editor.run("deleteCol")).toBe(true);
    expect(columnsOf(index)).toBe(2);
  });

  it("can be edited again after it was saved: the new row is part of the table the next time it is read", () => {
    load();
    expect(caretIn(index, "header")).toBe(true);
    editor.run("rowAfter");
    const saved = editor.text();
    const next = buildBlockMap(saved, mockParseBlocks(saved));
    const report = editor.load({ map: next });
    expect(report.reasons.size).toBe(0);
    const again = next.blocks.findIndex((b) => b.kind === "table");
    expect(rowsOf(again)).toBe(4);
    expect(caretIn(again, "body")).toBe(true);
    expect(editor.run("rowAfter")).toBe(true);
    expect(rowsOf(again)).toBe(5);
    expect(editor.run("deleteRow")).toBe(true);
    expect(rowsOf(again)).toBe(4);
  });
});

// ---- the keys of a table ---------------------------------------------------------------------------------------------

describe("the keys of a table", () => {
  const INTRO_LINE = "Intro paragraph.";
  const OUTRO_LINE = "Outro paragraph.";
  const GRID = [
    "| Name | Kind | Size |",
    "|---|---|---|",
    "| Alder | tree | tall |",
    "| Birch | tree | slim |",
    "| Cedar | tree | huge |",
  ].join("\n");
  const text = `${INTRO_LINE}\n\n${GRID}\n\n${OUTRO_LINE}\n`;
  const map = buildBlockMap(text, mockParseBlocks(text));
  const index = map.blocks.findIndex((b) => b.kind === "table");

  const load = (source = text): void => {
    const next = source === text ? map : buildBlockMap(source, mockParseBlocks(source));
    const report = editor.load({ map: next });
    expect(report.reasons.size, "every block is editable").toBe(0);
  };
  const cellText = (row: number, col: number): string =>
    editor.core.view.state.doc.child(index).child(row).child(col).textContent;
  const unchanged = (): void => {
    expect(editor.text()).toBe(text);
    expect(editor.status()).toEqual({ dirty: false, changed: 0 });
  };

  beforeEach(() => load());

  describe("Enter", () => {
    it("moves from the header row to the same column of the first body row", () => {
      caretInCell(index, 0, 1);
      expect(press("Enter")).toBe(true);
      expect(caretCell()).toEqual({ row: 1, col: 1 });
      expect(editor.core.view.state.selection.empty).toBe(true);
      unchanged();
    });

    it("moves from a body row to the same column of the row below", () => {
      caretInCell(index, 2, 2);
      expect(press("Enter")).toBe(true);
      expect(caretCell()).toEqual({ row: 3, col: 2 });
      expect(rowsOf(index)).toBe(4);
      unchanged();
    });

    it("puts the caret at the end of the text of the cell below", () => {
      caretInCell(index, 1, 0);
      press("Enter");
      const { $head } = editor.core.view.state.selection;
      expect($head.parent.textContent).toBe("Birch");
      expect($head.parentOffset).toBe("Birch".length);
    });

    it("moves on from the middle of a cell's text", () => {
      caretInCell(index, 1, 1);
      const { view } = editor.core;
      view.dispatch(
        view.state.tr.setSelection(
          Selection.near(view.state.doc.resolve(view.state.selection.from - 2), -1),
        ),
      );
      expect(press("Enter")).toBe(true);
      expect(caretCell()).toEqual({ row: 2, col: 1 });
      unchanged();
    });

    it("adds a row below the last row and moves into it, in the same column", () => {
      for (const col of [0, 1, 2]) {
        load();
        caretInCell(index, 3, col);
        expect(press("Enter"), `column ${col}`).toBe(true);
        expect(rowsOf(index)).toBe(5);
        expect(caretCell()).toEqual({ row: 4, col });
        for (let c = 0; c < 3; c++) expect(cellText(4, c)).toBe("");
        expect(cellText(3, col)).not.toBe("");
      }
    });

    it("adds one row for each Enter in the last row, as it moves down", () => {
      caretInCell(index, 3, 0);
      press("Enter");
      press("Enter");
      expect(rowsOf(index)).toBe(6);
      expect(caretCell()).toEqual({ row: 5, col: 0 });
    });

    it("changes only the table block when it adds a row", () => {
      caretInCell(index, 3, 1);
      press("Enter");
      const after = editor.text();
      const gridAt = text.indexOf(GRID);
      expect(after.slice(0, gridAt)).toBe(text.slice(0, gridAt));
      expect(after.endsWith(text.slice(gridAt + GRID.length))).toBe(true);
      expect(editor.status()).toMatchObject({ dirty: true, changed: 1 });
    });

    it("is one undo step: the row goes, the text is as it was", () => {
      caretInCell(index, 3, 2);
      press("Enter");
      expect(rowsOf(index)).toBe(5);
      const { view } = editor.core;
      expect(undo(view.state, view.dispatch)).toBe(true);
      expect(rowsOf(index)).toBe(4);
      unchanged();
    });

    it("is an undo step of its own, apart from the typing before it", () => {
      caretInCell(index, 3, 2);
      const { view } = editor.core;
      view.dispatch(view.state.tr.insertText("!"));
      press("Enter");
      expect(rowsOf(index)).toBe(5);
      expect(undo(view.state, view.dispatch)).toBe(true);
      expect(rowsOf(index)).toBe(4);
      expect(cellText(3, 2)).toBe("huge!");
    });

    it("is never taken while an input method is composing", () => {
      caretInCell(index, 3, 1);
      expect(pressTableKeys("Enter")).toBe(true);
      load();
      caretInCell(index, 3, 1);
      expect(pressTableKeys("Enter", { isComposing: true })).toBe(false);
      expect(pressTableKeys("Enter", { keyCode: 229 })).toBe(false);
      expect(rowsOf(index)).toBe(4);
      expect(caretCell()).toEqual({ row: 3, col: 1 });
      unchanged();
    });

    it("does nothing to the table when the editor is in the middle of a composition", () => {
      caretInCell(index, 3, 1);
      const { dom } = editor.core.view;
      dom.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
      dom.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      dom.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true }));
      expect(rowsOf(index)).toBe(4);
      expect(editor.core.view.state.doc.childCount).toBe(3);
      unchanged();
    });

    it("does not change Enter outside a table: a paragraph splits, a list item splits", () => {
      const { view } = editor.core;
      view.dispatch(view.state.tr.setSelection(Selection.near(view.state.doc.resolve(4), 1)));
      expect(editor.format().table).toBe(false);
      const count = view.state.doc.childCount;
      expect(press("Enter")).toBe(true);
      expect(view.state.doc.childCount).toBe(count + 1);
      expect(view.state.doc.child(0).textContent).toBe(INTRO_LINE.slice(0, 3));
      expect(view.state.doc.child(1).textContent).toBe(INTRO_LINE.slice(3));
      expect(rowsOf(index + 1)).toBe(4);

      load("- one\n- two\n");
      view.dispatch(view.state.tr.setSelection(Selection.atEnd(view.state.doc)));
      expect(press("Enter")).toBe(true);
      expect(view.state.doc.child(0).childCount).toBe(3);
    });
  });

  describe("Shift+Enter", () => {
    it("moves to the same column of the row above", () => {
      caretInCell(index, 3, 2);
      expect(press("Enter", { shiftKey: true })).toBe(true);
      expect(caretCell()).toEqual({ row: 2, col: 2 });
      unchanged();
    });

    it("moves from the first body row to the header row", () => {
      caretInCell(index, 1, 1);
      expect(press("Enter", { shiftKey: true })).toBe(true);
      expect(caretCell()).toEqual({ row: 0, col: 1 });
      unchanged();
    });

    it("does nothing in the header row, and puts no line break in the cell", () => {
      caretInCell(index, 0, 1);
      const before = editor.core.view.state.selection.from;
      expect(press("Enter", { shiftKey: true })).toBe(true);
      expect(caretCell()).toEqual({ row: 0, col: 1 });
      expect(editor.core.view.state.selection.from).toBe(before);
      unchanged();
    });

    it("is never taken while an input method is composing", () => {
      caretInCell(index, 2, 0);
      expect(pressTableKeys("Enter", { shiftKey: true, isComposing: true })).toBe(false);
      expect(caretCell()).toEqual({ row: 2, col: 0 });
    });

    it("does not change Shift+Enter outside a table: it is a line break", () => {
      const { view } = editor.core;
      view.dispatch(view.state.tr.setSelection(Selection.near(view.state.doc.resolve(4), 1)));
      expect(press("Enter", { shiftKey: true })).toBe(true);
      let breaks = 0;
      view.state.doc.child(0).forEach((node) => {
        if (node.type.name === "hardbreak") breaks++;
      });
      expect(breaks).toBe(1);
    });
  });

  describe("Ctrl+Enter", () => {
    it("moves to the paragraph right after the table without adding one", () => {
      caretInCell(index, 2, 1);
      const { view } = editor.core;
      const count = view.state.doc.childCount;
      expect(press("Enter", { ctrlKey: true })).toBe(true);
      expect(view.state.doc.childCount).toBe(count);
      expect(caretCell()).toBeNull();
      const { $head } = view.state.selection;
      expect($head.parent.textContent).toBe(OUTRO_LINE);
      expect($head.parentOffset).toBe(0);
      unchanged();
    });

    it("works from the header row and from the last cell", () => {
      for (const [row, col] of [
        [0, 0],
        [3, 2],
      ]) {
        load();
        caretInCell(index, row, col);
        expect(press("Enter", { ctrlKey: true })).toBe(true);
        expect(caretCell()).toBeNull();
        expect(editor.core.view.state.selection.$head.parent.textContent).toBe(OUTRO_LINE);
      }
    });

    it("adds an empty paragraph after a table that ends the document, and puts the caret in it", () => {
      const source = `${INTRO_LINE}\n\n${GRID}\n`;
      load(source);
      caretInCell(index, 3, 2);
      const { view } = editor.core;
      const count = view.state.doc.childCount;
      expect(press("Enter", { ctrlKey: true })).toBe(true);
      expect(view.state.doc.childCount).toBe(count + 1);
      const { $head } = view.state.selection;
      expect($head.parent.type.name).toBe("paragraph");
      expect($head.parent.content.size).toBe(0);
      expect($head.index(0)).toBe(count);
      expect(rowsOf(index)).toBe(4);
      // typing goes below the table
      view.dispatch(view.state.tr.insertText("Below"));
      expect(editor.text().startsWith(source.slice(0, source.indexOf(GRID) + GRID.length))).toBe(
        true,
      );
      expect(editor.text()).toContain("Below");
    });

    it("adds an empty paragraph between the table and a block that is not a paragraph", () => {
      const source = `${INTRO_LINE}\n\n${GRID}\n\n## Next\n`;
      load(source);
      caretInCell(index, 1, 0);
      const { view } = editor.core;
      const count = view.state.doc.childCount;
      expect(press("Enter", { ctrlKey: true })).toBe(true);
      expect(view.state.doc.childCount).toBe(count + 1);
      const { $head } = view.state.selection;
      expect($head.parent.type.name).toBe("paragraph");
      expect($head.parent.content.size).toBe(0);
      expect($head.index(0)).toBe(index + 1);
      expect(view.state.doc.child(index + 2).type.name).toBe("heading");
    });

    it("does nothing outside a table", () => {
      const { view } = editor.core;
      view.dispatch(view.state.tr.setSelection(Selection.near(view.state.doc.resolve(4), 1)));
      expect(press("Enter", { ctrlKey: true })).toBe(false);
      unchanged();
    });

    it("is never taken while an input method is composing", () => {
      caretInCell(index, 2, 0);
      expect(pressTableKeys("Enter", { ctrlKey: true, isComposing: true })).toBe(false);
      expect(caretCell()).toEqual({ row: 2, col: 0 });
    });
  });

  describe("Tab and Shift+Tab", () => {
    it("move to the next and the previous cell, and Tab in the last cell adds a row", () => {
      caretInCell(index, 1, 0);
      expect(press("Tab")).toBe(true);
      expect(caretCell()).toEqual({ row: 1, col: 1 });
      expect(press("Tab", { shiftKey: true })).toBe(true);
      expect(caretCell()).toEqual({ row: 1, col: 0 });
      caretInCell(index, 3, 2);
      expect(press("Tab")).toBe(true);
      expect(rowsOf(index)).toBe(5);
      expect(caretCell()).toEqual({ row: 4, col: 0 });
    });
  });
});
// ---- every table of the corpus ------------------------------------------------------------------------------------

const MAX_TABLES_PER_DOC = 3;
/** How many commands ran on a table of the corpus: a corpus whose tables were all locked would pass the checks below. */
let commandsRun = 0;
const HEADER_COMMANDS: CommandId[] = ["rowAfter", "colBefore", "colAfter", "deleteCol"];
const BODY_COMMANDS: CommandId[] = [
  "rowBefore",
  "rowAfter",
  "deleteRow",
  "colBefore",
  "colAfter",
  "deleteCol",
];

for (const splitter of splitters.filter((s) => s.docs.length > 0)) {
  describe(`the table commands with the ${splitter.name}`, () => {
    const withTables = splitter.docs
      .map((doc) => ({ doc, map: buildBlockMap(doc.text, splitter.parse(doc)) }))
      .filter(({ map }) => map.blocks.some((b) => b.kind === "table"));

    it("has tables to try", () => {
      expect(withTables.length).toBeGreaterThan(0);
    });

    it.each(withTables.map(({ doc, map }) => [doc.name, doc.text, map] as const))(
      "%s: every command rewrites the table and nothing around it",
      (_name, text, map) => {
        const report = editor.load({ map });
        const starts = blockStarts(map);
        const tables = map.blocks
          .map((b, i) => (b.kind === "table" && !report.reasons.has(i) ? i : -1))
          .filter((i) => i >= 0)
          .slice(0, MAX_TABLES_PER_DOC);
        for (const i of tables) {
          const end = starts[i] + map.blocks[i].text.length;
          for (const [row, commands] of [
            ["header", HEADER_COMMANDS],
            ["body", BODY_COMMANDS],
          ] as const) {
            for (const command of commands) {
              editor.restart();
              if (!caretIn(i, row)) continue;
              if (command === "deleteCol" && columnsOf(i) < 2) continue;
              const what = `block ${i}, ${command} from the ${row} row`;
              expect(editor.run(command), what).toBe(true);
              const after = editor.text();
              expect(after.slice(0, starts[i]), `${what}: bytes before`).toBe(
                text.slice(0, starts[i]),
              );
              expect(after.endsWith(text.slice(end)), `${what}: bytes after`).toBe(true);
              expect(editor.status().changed, `${what}: blocks changed`).toBe(1);
              commandsRun++;
            }
          }
          // Enter in the last cell adds a row, and that rewrites the table block and nothing else
          editor.restart();
          if (caretInLastCell(i)) {
            const what = `block ${i}, Enter in the last cell`;
            expect(press("Enter"), what).toBe(true);
            const after = editor.text();
            expect(after.slice(0, starts[i]), `${what}: bytes before`).toBe(
              text.slice(0, starts[i]),
            );
            expect(after.endsWith(text.slice(end)), `${what}: bytes after`).toBe(true);
            expect(editor.status().changed, `${what}: blocks changed`).toBe(1);
            commandsRun++;
          }
        }
      },
      60_000,
    );
  });
}

describe("the corpus", () => {
  it("had enough tables to run the commands on", () => {
    expect(commandsRun).toBeGreaterThan(100);
  });
});
