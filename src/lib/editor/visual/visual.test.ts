// @vitest-environment jsdom
import { redo, undo } from "@milkdown/kit/prose/history";
import { Fragment, Slice } from "@milkdown/kit/prose/model";
import { NodeSelection, TextSelection } from "@milkdown/kit/prose/state";
import { splitBlock } from "@milkdown/kit/prose/commands";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { mockParseBlocks } from "../../mock/mockBlocks";
import { buildBlockMap } from "../blockModel";
import "./jsdomShims";
import { VisualEditor, type VisualInput } from "./editor";

let editor: VisualEditor;
let changes = 0;
const scroller = document.createElement("div");

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.appendChild(host);
  editor = await VisualEditor.create({
    host,
    label: "test",
    scroll: false,
    scroller,
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
      changed: () => {
        changes++;
      },
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

const load = (text: string, extra: Partial<VisualInput> = {}) =>
  editor.load({ map: buildBlockMap(text, mockParseBlocks(text)), ...extra });

/** The position inside the n-th top-level node, `offset` characters into its first text block. */
function inside(index: number, offset = 0): number {
  const { doc } = editor.core.view.state;
  let pos = 0;
  for (let i = 0; i < index; i++) pos += doc.child(i).nodeSize;
  return pos + 1 + offset;
}
function typeAt(pos: number, text: string): void {
  const { view } = editor.core;
  view.dispatch(view.state.tr.insertText(text, pos));
}
const DOC = "# Title\n\nFirst paragraph.\n\n\n\nSecond paragraph.\n\n- a\n- b\n";

describe("loading", () => {
  it("shows every block of a plain document as an editable node, and saves it back untouched", () => {
    const report = load(DOC);
    expect(report.editable).toBe(4);
    expect(editor.text()).toBe(DOC);
    expect(editor.status()).toEqual({ dirty: false, changed: 0 });
  });

  it("locks the blocks it must not rewrite", () => {
    const text =
      "---\ntitle: x\n---\n\n# T\n\n<div>raw</div>\n\n```mermaid\ngraph LR\n```\n\nSee $a_b_c$ here.\n\n> [!NOTE]\n> hi\n\nText.\n";
    const report = load(text);
    expect(report.locked).toMatchObject({ kind: 3, diagram: 1, math: 1 });
    expect(report.editable).toBe(2);
    expect(editor.text()).toBe(text);
  });

  it("locks a block that uses a reference defined elsewhere", () => {
    const text =
      "See [the docs][d] and [^1].\n\nPlain.\n\n[d]: https://example.com\n\n[^1]: note\n";
    const report = load(text);
    expect(report.reasons.get(0)).toBe("reference");
    expect(editor.text()).toBe(text);
  });
});

describe("dirty tracking", () => {
  it("typing makes only that block dirty, and only its bytes change", () => {
    load(DOC);
    typeAt(inside(1, 5), "X");
    expect(editor.status()).toEqual({ dirty: true, changed: 1 });
    expect(editor.text()).toBe(DOC.replace("First paragraph.", "FirstX paragraph."));
  });

  it("undo back to the original clears dirtiness", () => {
    load(DOC);
    typeAt(inside(1, 5), "X");
    expect(editor.status().dirty).toBe(true);
    const { view } = editor.core;
    expect(undo(view.state, view.dispatch)).toBe(true);
    expect(editor.status()).toEqual({ dirty: false, changed: 0 });
    expect(editor.text()).toBe(DOC);
    expect(redo(view.state, view.dispatch)).toBe(true);
    expect(editor.status().dirty).toBe(true);
  });

  it("typing and deleting the same character is not a change", () => {
    load(DOC);
    typeAt(inside(1, 5), "X");
    const { view } = editor.core;
    view.dispatch(view.state.tr.delete(inside(1, 5), inside(1, 6)));
    expect(editor.status().dirty).toBe(false);
  });

  it("a split makes both halves dirty and keeps the other blocks' bytes", () => {
    load(DOC);
    const { view } = editor.core;
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, inside(1, 5))));
    splitBlock(view.state, view.dispatch);
    expect(editor.status().changed).toBe(2);
    // the second half is a new block, so it is one blank line from its neighbours (the two the author left are gone)
    expect(editor.text()).toBe("# Title\n\nFirst\n\nparagraph.\n\nSecond paragraph.\n\n- a\n- b\n");
  });

  it("an Enter at the end of a block adds an empty paragraph that is not written", () => {
    load(DOC);
    const { view } = editor.core;
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, inside(1, 16))));
    splitBlock(view.state, view.dispatch);
    expect(editor.text()).toBe(DOC);
    expect(editor.status().dirty).toBe(false);
  });

  it("deleting a block drops its bytes and keeps its neighbours' (and the blank lines they were given)", () => {
    load(DOC);
    const { view } = editor.core;
    const from = inside(1) - 1;
    view.dispatch(view.state.tr.delete(from, from + view.state.doc.child(1).nodeSize));
    expect(editor.status().dirty).toBe(true);
    expect(editor.text()).toBe("# Title\n\nSecond paragraph.\n\n- a\n- b\n");
  });

  it("a paragraph turned into a heading is still the same block, so the blank lines around it stay", () => {
    load(DOC);
    editor.core.view.dispatch(
      editor.core.view.state.tr.setSelection(
        TextSelection.create(editor.core.view.state.doc, inside(1, 2)),
      ),
    );
    editor.run("h2");
    expect(editor.text()).toBe(DOC.replace("First paragraph.", "## First paragraph."));
    expect(editor.status().changed).toBe(1);
  });

  it("toggling a task checkbox rewrites only that list", () => {
    const text = "Intro.\n\n- [ ] one\n- [x] two\n\nOutro.\n";
    load(text);
    const { view } = editor.core;
    let found = -1;
    view.state.doc.descendants((node, pos) => {
      if (found < 0 && node.type.name === "list_item") found = pos;
    });
    view.dispatch(
      view.state.tr.setNodeMarkup(found, undefined, {
        ...view.state.doc.nodeAt(found)?.attrs,
        checked: true,
      }),
    );
    expect(editor.text()).toBe("Intro.\n\n- [x] one\n- [x] two\n\nOutro.\n");
    expect(editor.status().changed).toBe(1);
  });

  it("a pasted copy of a block is new text and the original keeps its bytes", () => {
    load(DOC);
    const { view } = editor.core;
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, inside(2, 3))));
    // what the clipboard would hold: the second paragraph, as the editor copied it
    const copied = new Slice(Fragment.from(view.state.doc.child(2)), 0, 0);
    let pasted = copied;
    view.someProp("transformPasted", (f) => {
      pasted = f(pasted, view, false);
    });
    expect(pasted.content.child(0).attrs.tmBlock).toBeNull();
    view.dispatch(view.state.tr.insert(inside(1) - 1, pasted.content));
    // the copy is a new block (written by the editor); the originals keep their bytes
    expect(editor.text()).toBe(
      DOC.replace("First paragraph.", "Second paragraph.\n\nFirst paragraph."),
    );
    expect(editor.status().changed).toBe(1);
  });
});

describe("a selected locked block", () => {
  const RAW = "<div>raw</div>\n\ntext\n";
  const textInput = (text: string) => {
    const { view } = editor.core;
    const { from, to } = view.state.selection;
    return view.someProp("handleTextInput", (f) => f(view, from, to, text, () => view.state.tr));
  };

  it("is not replaced by typing: the text is swallowed and nothing changes", () => {
    load(RAW);
    const { view } = editor.core;
    view.dispatch(view.state.tr.setSelection(NodeSelection.create(view.state.doc, 0)));
    expect(textInput("z")).toBe(true);
    expect(editor.text()).toBe(RAW);
    expect(editor.status()).toEqual({ dirty: false, changed: 0 });
  });

  it("leaves typing in the text next to it alone", () => {
    load(RAW);
    const { view } = editor.core;
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, inside(1, 2))));
    expect(textInput("z")).toBeFalsy();
  });

  it("can still be deleted on purpose, and the deletion is undone like any other", () => {
    load(RAW);
    const { view } = editor.core;
    view.dispatch(
      view.state.tr.setSelection(NodeSelection.create(view.state.doc, 0)).deleteSelection(),
    );
    expect(editor.text()).toBe("text\n");
    expect(editor.status().dirty).toBe(true);
    expect(undo(view.state, view.dispatch)).toBe(true);
    expect(editor.text()).toBe(RAW);
  });
});

describe("unsaved edits that came from Source", () => {
  it("shows the blocks that differ from the saved file, and writes the text as it came", () => {
    const saved = "# T\n\nOne.\n\nTwo.\n";
    const working = "# T\n\nOne, changed *in Source*.\n\nTwo.\n";
    const base = buildBlockMap(saved, mockParseBlocks(saved));
    const next = buildBlockMap(working, mockParseBlocks(working));
    const unsaved = next.blocks.map((b) => !base.blocks.some((o) => o.text === b.text));
    editor.load({ map: next, unsaved, dirty: true });
    expect(editor.status()).toEqual({ dirty: true, changed: 1 });
    expect(editor.text()).toBe(working);
  });
});

describe("the caret", () => {
  it("maps a block to the line it starts on in the text a save would write", () => {
    load(DOC, { block: 2 });
    expect(editor.caretBlock()).toBe(2);
    // the heading is on line 0, "First paragraph." on 2, "Second paragraph." on 6
    expect(editor.caretLine()).toBe(6);
  });
});

describe("the Outline", () => {
  const TEXT =
    "# Title\n\nIntro.\n\n## Sub\n\n> ## Quoted\n\n- ### Listed\n\nSetext\n======\n\n## Fancy **bold** [link](x.md) ![alt text](a.png)\n\nEnd.\n";

  /** Where each top-level heading is on screen, as the layout would say (jsdom has none). */
  function layout(
    tops: number[],
    box: { top?: number; height: number; scrollTop?: number; scrollHeight: number },
  ) {
    const { doc } = editor.core.view.state;
    let at = 0;
    let next = 0;
    doc.forEach((node, pos) => {
      if (node.type.name !== "heading") return;
      const el = editor.core.view.nodeDOM(pos) as HTMLElement;
      const top = tops[next++];
      vi.spyOn(el, "getBoundingClientRect").mockReturnValue(new DOMRect(0, top, 100, 20));
      at++;
    });
    expect(at).toBe(tops.length);
    vi.spyOn(scroller, "getBoundingClientRect").mockReturnValue(
      new DOMRect(0, box.top ?? 0, 100, box.height),
    );
    Object.defineProperty(scroller, "clientHeight", { value: box.height, configurable: true });
    Object.defineProperty(scroller, "scrollHeight", {
      value: box.scrollHeight,
      configurable: true,
    });
    scroller.scrollTop = box.scrollTop ?? 0;
  }

  it("lists the top-level headings with the text a reader sees", () => {
    load(TEXT);
    expect(editor.headings()).toEqual([
      { level: 1, text: "Title" },
      { level: 2, text: "Sub" },
      { level: 1, text: "Setext" },
      { level: 2, text: "Fancy bold link alt text" },
    ]);
  });

  it("follows the document as it is edited", () => {
    load("# Title\n\nIntro.\n");
    const { view } = editor.core;
    expect(editor.headings()).toHaveLength(1);
    view.dispatch(
      view.state.tr.setNodeMarkup(inside(1) - 1, view.state.schema.nodes.heading, { level: 3 }),
    );
    expect(editor.headings()).toEqual([
      { level: 1, text: "Title" },
      { level: 3, text: "Intro." },
    ]);
    typeAt(inside(1, 6), " now");
    expect(editor.headings()[1].text).toBe("Intro. now");
  });

  it("has no headings in a document without any", () => {
    load("Just text.\n\n- and a list\n");
    expect(editor.headings()).toEqual([]);
    expect(editor.currentHeading()).toBeNull();
  });

  it("scrolls the editor so the heading is 8 px under the top", () => {
    load(TEXT);
    layout([-900, 400, 1300, 2000], { top: 100, height: 600, scrollTop: 50, scrollHeight: 5000 });
    const scrollTo = vi.fn();
    scroller.scrollTo = scrollTo as unknown as typeof scroller.scrollTo;
    editor.scrollToHeading(1);
    // the heading is 300 px under the pane's top edge now (400 - 100); the pane is at 50
    expect(scrollTo).toHaveBeenCalledWith({ top: 50 + 300 - 8, behavior: "instant" });
    scrollTo.mockClear();
    editor.scrollToHeading(0);
    expect(scrollTo).toHaveBeenCalledWith({ top: 0, behavior: "instant" }); // never above the start
    scrollTo.mockClear();
    editor.scrollToHeading(9);
    expect(scrollTo).not.toHaveBeenCalled();
  });

  it("names the section in view: the last heading above the top-third line", () => {
    load(TEXT);
    layout([-300, 120, 900, 1500], { height: 600, scrollHeight: 5000 });
    expect(editor.currentHeading()).toBe(1); // the line is at 200
    layout([300, 700, 900, 1500], { height: 600, scrollHeight: 5000 });
    expect(editor.currentHeading()).toBeNull(); // above the first heading
  });

  it("counts the last heading in view as the section at the very end of the document", () => {
    load(TEXT);
    layout([-900, -400, 100, 500], { height: 600, scrollTop: 4400, scrollHeight: 5000 });
    expect(editor.currentHeading()).toBe(3);
  });

  it("tells when the editor scrolls and when the person does", () => {
    const seen: boolean[] = [];
    const stop = editor.onViewport((byUser) => seen.push(byUser));
    scroller.dispatchEvent(new Event("scroll"));
    scroller.dispatchEvent(new Event("wheel"));
    stop();
    scroller.dispatchEvent(new Event("scroll"));
    expect(seen).toEqual([false, true]);
  });
});

it("reports changes", () => {
  expect(changes).toBeGreaterThan(0);
});
