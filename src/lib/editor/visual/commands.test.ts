// @vitest-environment jsdom
import { closeHistory, undo } from "@milkdown/kit/prose/history";
import { NodeSelection, TextSelection } from "@milkdown/kit/prose/state";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { mockParseBlocks } from "../../mock/mockBlocks";
import { buildBlockMap } from "../blockModel";
import { VisualEditor } from "./editor";
import "./jsdomShims";

let editor: VisualEditor;
let slashes: ({ query: string } | null)[] = [];

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.appendChild(host);
  editor = await VisualEditor.create({
    host,
    label: "test",
    scroll: false,
    imageUrl: (src) => `asset://${src}`,
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
      slash: (state) => void slashes.push(state),
      slashKey: () => false,
    },
  });
});
afterAll(async () => {
  await editor.destroy();
});

const load = (text: string): void => {
  editor.load({ map: buildBlockMap(text, mockParseBlocks(text)) });
  slashes = [];
};

/** The position `offset` characters into the text of the n-th top-level node. */
function inside(index: number, offset = 0): number {
  const { doc } = editor.core.view.state;
  let pos = 0;
  for (let i = 0; i < index; i++) pos += doc.child(i).nodeSize;
  return pos + 1 + offset;
}
function caret(index: number, offset = 0): void {
  const { view } = editor.core;
  view.dispatch(
    view.state.tr.setSelection(TextSelection.create(view.state.doc, inside(index, offset))),
  );
}
function select(from: number, to: number): void {
  const { view } = editor.core;
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, from, to)));
}

describe("the toolbar commands", () => {
  it("toggle bold, italic, strikethrough and inline code on the selection, and say so in format()", () => {
    load("Some plain text here.\n");
    select(inside(0, 5), inside(0, 10));
    for (const [command, mark] of [
      ["bold", "bold"],
      ["italic", "italic"],
      ["strike", "strike"],
      ["code", "code"],
    ] as const) {
      expect(editor.format()[mark]).toBe(false);
      editor.run(command);
      expect(editor.format()[mark], command).toBe(true);
      editor.run(command);
      expect(editor.format()[mark], command).toBe(false);
    }
    expect(editor.status().dirty).toBe(false);
    editor.run("bold");
    expect(editor.text()).toBe("Some **plain** text here.\n");
  });

  it("turn a paragraph into a heading, and back when the same heading is chosen again", () => {
    load("Title text\n");
    caret(0, 2);
    editor.run("h3");
    expect(editor.format().block).toBe("h3");
    expect(editor.text()).toBe("### Title text\n");
    editor.run("h3");
    expect(editor.format().block).toBe("paragraph");
    expect(editor.text()).toBe("Title text\n");
    expect(editor.status().dirty).toBe(false);
  });

  it("toggle lists: wrap, switch kind, take out", () => {
    load("one\n\ntwo\n");
    caret(0, 1);
    editor.run("bullet");
    expect(editor.format().list).toBe("bullet");
    expect(editor.text()).toBe("- one\n\ntwo\n");
    editor.run("ordered");
    expect(editor.format().list).toBe("ordered");
    expect(editor.text()).toBe("1. one\n\ntwo\n");
    editor.run("ordered");
    expect(editor.format().list).toBeNull();
    expect(editor.text()).toBe("one\n\ntwo\n");
  });

  it("make a task list from a paragraph or a bullet list, and a bullet list again from a task list", () => {
    load("todo\n");
    caret(0, 1);
    editor.run("task");
    expect(editor.format().list).toBe("task");
    expect(editor.text()).toBe("- [ ] todo\n");
    editor.run("task");
    expect(editor.format().list).toBe("bullet");
    expect(editor.text()).toBe("- todo\n");
  });

  it("toggle a quote and a code block", () => {
    load("say this\n");
    caret(0, 2);
    editor.run("quote");
    expect(editor.format().quote).toBe(true);
    expect(editor.text()).toBe("> say this\n");
    editor.run("quote");
    expect(editor.format().quote).toBe(false);
    expect(editor.text()).toBe("say this\n");
    editor.run("codeBlock");
    expect(editor.format().block).toBe("code");
    expect(editor.text()).toBe("```\nsay this\n```\n");
    editor.setLanguage("ts");
    expect(editor.format().language).toBe("ts");
    expect(editor.text()).toBe("```ts\nsay this\n```\n");
    editor.run("codeBlock");
    expect(editor.format().block).toBe("paragraph");
  });

  it("insert a rule", () => {
    load("above\n");
    caret(0, 5);
    editor.run("rule");
    expect(editor.text()).toContain("---");
  });

  it("insert a 3 x 3 table written without alignment, and edit it with the table menu", () => {
    load("\n");
    const { view } = editor.core;
    view.dispatch(view.state.tr.setSelection(TextSelection.atStart(view.state.doc)));
    expect(editor.run("table")).toBe(true);
    expect(editor.format().table).toBe(true);
    const rows = (): number => {
      let n = 0;
      editor.core.view.state.doc.descendants((node) => {
        if (node.type.name === "table_row" || node.type.name === "table_header_row") n++;
      });
      return n;
    };
    expect(rows()).toBe(3);
    // no `:-` alignment marks: like the tables read from a file
    expect(editor.text()).not.toContain(":");
    // the caret starts in the header row, which gets no row above it and cannot be deleted (a table needs its header)
    expect(editor.format().tableHeader).toBe(true);
    expect(editor.run("rowBefore")).toBe(false);
    expect(editor.run("deleteRow")).toBe(false);
    expect(rows()).toBe(3);
    editor.run("rowAfter");
    expect(rows()).toBe(4);
    // now in a body row: a row above, and the row itself can go
    const { view: v } = editor.core;
    let body = -1;
    v.state.doc.descendants((node, pos) => {
      if (body < 0 && node.type.name === "table_row") body = pos;
    });
    v.dispatch(v.state.tr.setSelection(TextSelection.create(v.state.doc, body + 3)));
    expect(editor.format().tableHeader).toBe(false);
    editor.run("rowBefore");
    expect(rows()).toBe(5);
    editor.run("deleteRow");
    expect(rows()).toBe(4);
    editor.run("colAfter");
    editor.run("colBefore");
    editor.run("deleteCol");
    editor.run("deleteTable");
    expect(editor.format().table).toBe(false);
  });

  it("do not run the table commands outside a table", () => {
    load("no table here\n");
    caret(0, 1);
    for (const command of [
      "rowBefore",
      "rowAfter",
      "colBefore",
      "colAfter",
      "deleteRow",
      "deleteCol",
      "deleteTable",
    ] as const) {
      expect(editor.run(command), command).toBe(false);
    }
    expect(editor.status().dirty).toBe(false);
  });

  it("do nothing on a locked block", () => {
    load("<div>raw</div>\n\ntext\n");
    const { view } = editor.core;
    view.dispatch(view.state.tr.setSelection(NodeSelection.create(view.state.doc, 0)));
    expect(editor.format().atom).toBe(true);
    expect(editor.run("bold")).toBe(false);
    expect(editor.run("h1")).toBe(false);
    expect(editor.status().dirty).toBe(false);
  });

  it("insert an image with its path as written, drawn from the address the app gives it", () => {
    load("pic: x\n");
    caret(0, 5);
    editor.insertImage("assets/shot.png");
    expect(editor.text()).toBe("pic: ![](assets/shot.png)x\n");
    const img = editor.core.view.dom.querySelector("img");
    expect(img?.getAttribute("src")).toBe("asset://assets/shot.png");
    expect(img?.getAttribute("data-src")).toBe("assets/shot.png");
  });
});

describe("links", () => {
  it("make a link from the selected text, change its target and take it off", () => {
    load("see the docs now\n");
    select(inside(0, 8), inside(0, 12));
    const target = editor.linkTarget();
    expect(target).toMatchObject({ text: "docs", href: "", existing: false });
    expect(editor.applyLink(target, { text: "docs", href: "../docs.md" })).toBe(true);
    expect(editor.text()).toBe("see the [docs](../docs.md) now\n");
    expect(editor.format().link).toBe(true);

    caret(0, 10);
    const existing = editor.linkTarget();
    expect(existing).toMatchObject({ text: "docs", href: "../docs.md", existing: true });
    editor.applyLink(existing, { text: "documents", href: "#top" });
    expect(editor.text()).toBe("see the [documents](#top) now\n");

    caret(0, 12);
    editor.applyLink(editor.linkTarget(), { text: "documents", href: "" });
    expect(editor.text()).toBe("see the documents now\n");
    expect(editor.status().dirty).toBe(true);
  });

  it("make a link out of nothing when there is no selection, the text being the target when none is given", () => {
    load("see .\n");
    caret(0, 4);
    editor.applyLink(editor.linkTarget(), { text: "", href: "https://example.com" });
    expect(editor.text()).toBe("see <https://example.com>.\n");
  });
});

describe("the slash menu", () => {
  it("opens for a slash alone in a paragraph, follows what is typed, and closes with the text", () => {
    load("\n");
    const { view } = editor.core;
    view.dispatch(
      view.state.tr.setSelection(TextSelection.atStart(view.state.doc)).insertText("/"),
    );
    expect(slashes.at(-1)).toMatchObject({ query: "" });
    view.dispatch(view.state.tr.insertText("quo"));
    expect(slashes.at(-1)).toMatchObject({ query: "quo" });
    view.dispatch(view.state.tr.insertText(" "));
    expect(slashes.at(-1)).toBeNull();
  });

  it("does not open for a slash inside a sentence or a table cell", () => {
    load("a sentence\n");
    caret(0, 10);
    const { view } = editor.core;
    view.dispatch(view.state.tr.insertText(" /"));
    expect(slashes.filter(Boolean)).toHaveLength(0);
  });

  it("runs a command in place of the typed text", () => {
    load("\n");
    const { view } = editor.core;
    view.dispatch(
      view.state.tr.setSelection(TextSelection.atStart(view.state.doc)).insertText("/h2"),
    );
    editor.runSlash("h2");
    expect(editor.format().block).toBe("h2");
    expect(view.state.doc.child(0).textContent).toBe("");
  });

  it("stays closed after Esc until the slash is gone", () => {
    load("\n");
    const { view } = editor.core;
    view.dispatch(
      view.state.tr.setSelection(TextSelection.atStart(view.state.doc)).insertText("/"),
    );
    editor.dismissSlash();
    slashes = [];
    view.dispatch(view.state.tr.insertText("x"));
    expect(slashes.filter(Boolean)).toHaveLength(0);
    // delete the whole text, then a new slash opens it again
    view.dispatch(view.state.tr.delete(1, view.state.doc.child(0).nodeSize - 1));
    view.dispatch(view.state.tr.insertText("/"));
    expect(slashes.at(-1)).toMatchObject({ query: "" });
  });
});

describe("the caret", () => {
  it("never lands in a locked block when a document opens on one", () => {
    const text = "---\ntitle: t\n---\n\n# Heading\n";
    editor.load({ map: buildBlockMap(text, mockParseBlocks(text)), block: 0 });
    expect(editor.format().atom).toBe(false);
    expect(editor.format().block).toBe("h1");
    expect(editor.caretBlock()).toBe(1);
  });

  it("goes at the end of a block when asked", () => {
    const text = "# Title\n\nbody\n";
    editor.load({ map: buildBlockMap(text, mockParseBlocks(text)), block: 0, atEnd: true });
    editor.insertImage("x.png");
    expect(editor.text()).toBe("# Title![](x.png)\n\nbody\n");
  });

  it("reports the line of the block it is in, and of a block asked for", () => {
    const text = "# T\n\none\n\ntwo\n";
    editor.load({ map: buildBlockMap(text, mockParseBlocks(text)), block: 1 });
    expect(editor.caretLine()).toBe(2);
    expect(editor.lineOfBlock(2)).toBe(4);
    expect(editor.lineOfBlock(null)).toBe(2);
  });
});

describe("rebase", () => {
  it("starts over from the saved blocks, clears the bars and keeps the selection", () => {
    const text = "# T\n\nsome words here\n";
    editor.load({ map: buildBlockMap(text, mockParseBlocks(text)), block: 1 });
    const { view } = editor.core;
    view.dispatch(view.state.tr.insertText("X", inside(1, 4)));
    expect(editor.status()).toEqual({ dirty: true, changed: 1 });
    const saved = editor.text();
    const at = view.state.selection.from;
    editor.rebase(buildBlockMap(saved, mockParseBlocks(saved)));
    expect(editor.status()).toEqual({ dirty: false, changed: 0 });
    expect(editor.text()).toBe(saved);
    expect(view.state.selection.from).toBe(at);
  });

  it("does not replace the document: the focus stays and undo goes back past the save", () => {
    const text = "# T\n\nsome words here\n";
    load(text);
    const { view } = editor.core;
    caret(1, 4);
    view.dispatch(view.state.tr.insertText("X"));
    view.focus();
    expect(view.hasFocus()).toBe(true);
    const doc = view.state.doc;
    const saved = editor.text();
    editor.rebase(buildBlockMap(saved, mockParseBlocks(saved)));
    // the nodes of the editor are the ones it had: nothing was loaded again
    expect(view.state.doc.child(1)).toBe(doc.child(1));
    expect(view.hasFocus()).toBe(true);
    // typing goes on after the save (a pause, so a new undo step), and Ctrl+Z walks back through what was typed before it
    view.dispatch(closeHistory(view.state.tr).insertText("Y"));
    expect(editor.text()).toBe("# T\n\nsomeXY words here\n");
    expect(undo(view.state, view.dispatch)).toBe(true);
    expect(editor.text()).toBe(saved);
    expect(editor.status()).toEqual({ dirty: false, changed: 0 });
    expect(undo(view.state, view.dispatch)).toBe(true);
    expect(editor.text()).toBe(text);
    expect(editor.status()).toEqual({ dirty: true, changed: 1 });
  });

  it("keeps the document as it is when the save does not write what the person has typed (a space at the end)", () => {
    // Markdown drops a space at the end of a paragraph or a heading, so the saved text has none: the save says "alpha", the
    // person has "alpha " and goes on typing. Neither the space nor the focus may be lost, and nothing is loaded again.
    const text = "# T\n\nalpha\n";
    load(text);
    const { view } = editor.core;
    caret(1, 5);
    view.dispatch(view.state.tr.insertText(" "));
    view.focus();
    const doc = view.state.doc;
    const saved = editor.text();
    editor.rebase(buildBlockMap(saved, mockParseBlocks(saved)));
    expect(view.state.doc.child(1)).toBe(doc.child(1));
    expect(view.hasFocus()).toBe(true);
    expect(view.state.doc.child(1).textContent).toBe("alpha ");
    // nothing is unsaved after the save, so another autosave is not asked for
    expect(editor.status()).toEqual({ dirty: false, changed: 0 });
    view.dispatch(view.state.tr.insertText("beta"));
    expect(view.state.doc.child(1).textContent).toBe("alpha beta");
    expect(editor.text()).toBe("# T\n\nalpha beta\n");
    expect(editor.status()).toEqual({ dirty: true, changed: 1 });
  });

  it("keeps a space at the end of a heading and of a list item through a save too", () => {
    const text = "# Title\n\n- one\n- two\n";
    load(text);
    const { view } = editor.core;
    caret(0, 5);
    view.dispatch(view.state.tr.insertText(" "));
    const before = view.state.doc;
    const saved = editor.text();
    editor.rebase(buildBlockMap(saved, mockParseBlocks(saved)));
    expect(view.state.doc.child(0)).toBe(before.child(0));
    expect(view.state.doc.child(0).textContent).toBe("Title ");
    expect(editor.status()).toEqual({ dirty: false, changed: 0 });
  });

  it("follows the blocks of the saved text when one was removed", () => {
    const text = "one\n\ntwo\n\nthree\n";
    load(text);
    const { view } = editor.core;
    view.dispatch(view.state.tr.delete(inside(1) - 1, inside(2) - 1));
    const saved = editor.text();
    expect(saved).toBe("one\n\nthree\n");
    editor.rebase(buildBlockMap(saved, mockParseBlocks(saved)));
    expect(editor.status()).toEqual({ dirty: false, changed: 0 });
    // the last paragraph is now the second block of the text: an edit there touches that block only
    view.dispatch(view.state.tr.insertText("!", inside(1, 5)));
    expect(editor.status()).toEqual({ dirty: true, changed: 1 });
    expect(editor.text()).toBe("one\n\nthree!\n");
  });

  it("forgets an empty paragraph the saved text does not have", () => {
    const text = "one\n\ntwo\n";
    load(text);
    const { view } = editor.core;
    view.dispatch(view.state.tr.delete(inside(0), inside(0, 3)));
    const saved = editor.text();
    expect(saved).toBe("two\n");
    editor.rebase(buildBlockMap(saved, mockParseBlocks(saved)));
    expect(editor.status()).toEqual({ dirty: false, changed: 0 });
    expect(editor.text()).toBe(saved);
  });

  it("does not load the document again even when the saved text has another number of blocks: it stays as the person has it", () => {
    const text = "# T\n\nsome words here\n";
    load(text);
    const { view } = editor.core;
    caret(1, 4);
    view.dispatch(view.state.tr.insertText("X"));
    view.focus();
    const doc = view.state.doc;
    const at = view.state.selection.from;
    const written = editor.text();
    // the saved text reads back as three blocks while the document has two
    const other = `${written}\nand one more\n`;
    editor.rebase(buildBlockMap(other, mockParseBlocks(other)));
    expect(view.state.doc).toBe(doc);
    expect(editor.text()).toBe(written);
    expect(view.hasFocus()).toBe(true);
    expect(view.state.selection.from).toBe(at);
    // what was saved is what the document is compared with: it is clean, and the next edit makes it unsaved again
    expect(editor.status().dirty).toBe(false);
    view.dispatch(closeHistory(view.state.tr).insertText("Y"));
    expect(editor.status().dirty).toBe(true);
    undo(view.state, view.dispatch);
    expect(editor.status().dirty).toBe(false);
  });
});
