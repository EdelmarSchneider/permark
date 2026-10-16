// @vitest-environment jsdom
// What saving an edited block writes, with the real editor and the block ranges the real parser found (recorded in
// `corpus/ranges.json`, so a definition that the parser leaves inside a block is really inside it, which the line-based mock
// splitter does not do). Each case is a document from the hostile set of the corpus. The checks look at what the saved text
// MEANS, not at what the editor wrote: a definition that is still there, blocks that stay apart, a link that ends where it did.

import { TextSelection } from "@milkdown/kit/prose/state";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { mockParseBlocks } from "../../mock/mockBlocks";
import { buildBlockMap } from "../blockModel";
import { everything, isCurrent, realRanges } from "../corpusDocs";
import type { LockReason } from "./core";
import { endOfText, startOf, startOfText } from "./docPositions";
import { VisualEditor } from "./editor";
import "./jsdomShims";
import { definitionsOf as definitionsIn, splitKinds, topLevel as topLevelIn } from "./treeChecks";

let editor: VisualEditor;
const definitionsOf = (text: string): string[] => definitionsIn(editor.core, text);
const topLevel = (text: string): string[] => topLevelIn(editor.core, text);
/** The links the app was asked to follow from inside a locked block. */
const followed: string[] = [];

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.appendChild(host);
  editor = await VisualEditor.create({
    host,
    label: "edits",
    scroll: false,
    imageUrl: (src) => src,
    locked: {
      // a raw HTML block is drawn as its own markup, as the reading view's HTML is; the rest as text
      render: (body, atom) => {
        if (atom.kind === "html") body.innerHTML = atom.source;
        else body.textContent = atom.source;
      },
      edit: () => {},
      link: (href) => followed.push(href),
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

/** Loads a document of the hostile set with the block ranges of the real parser. */
function open(name: string): { text: string; reasons: Map<number, LockReason> } {
  const doc = everything.find((d) => d.name === `hostile: ${name}`);
  if (!doc || !isCurrent(doc)) throw new Error(`no fixture with real ranges for "${name}"`);
  const report = editor.load({ map: buildBlockMap(doc.text, realRanges(doc)) });
  return { text: doc.text, reasons: report.reasons };
}

function typeAt(pos: number, text: string): void {
  const { view } = editor.core;
  view.dispatch(view.state.tr.insertText(text, pos));
}
function typeAtEnd(block: number, text: string): void {
  const end = endOfText(editor.core.view.state.doc, block);
  if (end === null) throw new Error(`block ${block} has no text`);
  typeAt(end, text);
}
function typeAtStart(block: number, text: string): void {
  const start = startOfText(editor.core.view.state.doc, block);
  if (start === null) throw new Error(`block ${block} has no text`);
  typeAt(start, text);
}
/** Puts the caret in a block and runs a toolbar command there. */
function convert(block: number, command: Parameters<VisualEditor["run"]>[0]): void {
  const { view } = editor.core;
  const at = startOfText(view.state.doc, block);
  if (at === null) throw new Error(`block ${block} has no text`);
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, at)));
  expect(editor.run(command), `${command} on block ${block}`).toBe(true);
}

// ---- B1: a definition inside a block --------------------------------------------------------------------------

describe("a link reference definition inside a block", () => {
  it.each([
    "definition directly before a paragraph",
    "definition inside a blockquote",
    "definition inside a list item",
  ])("is locked with the block that holds it, and the block that uses it (%s)", (name) => {
    const { text, reasons } = open(name);
    expect(reasons.get(0)).toBe("definition");
    expect(reasons.get(1)).toBe("reference");
    expect(editor.text()).toBe(text);
  });

  it("locks the block that uses a footnote defined inside a quote", () => {
    const { text, reasons } = open("footnote defined inside a blockquote");
    expect(reasons.get(1)).toBe("reference");
    expect(editor.text()).toBe(text);
  });

  it.each([
    "definition directly before a paragraph",
    "definition inside a blockquote",
    "definition inside a list item",
    "definition after a list item, indented",
    "footnote defined inside a blockquote",
  ])("survives an edit anywhere (%s)", (name) => {
    const { text } = open(name);
    const before = definitionsOf(text);
    expect(before.length).toBeGreaterThan(0);
    const count = editor.core.view.state.doc.childCount;
    for (let block = 0; block < count; block++) {
      editor.restart();
      if (endOfText(editor.core.view.state.doc, block) === null) continue;
      if (editor.core.view.state.doc.child(block).type.name === "tm_locked") continue;
      typeAtEnd(block, "X");
      expect(definitionsOf(editor.text()), `edit at the end of block ${block}`).toEqual(before);
    }
  });
});

// ---- footnotes: the blocks that refer to one, and the blocks that define one ----------------------------------

describe("footnotes", () => {
  const several =
    "footnote references in a paragraph, a list item and a table cell, a definition of several paragraphs";
  const between = "footnote definitions between the blocks, one before its reference";
  const NOTE_1 = "[^1]: Spices do not scale in a straight line.\n\n    Taste before adding more.";
  const NOTE_NAMED = "[^note]: A named note,\n    continued here.";

  const blocks = (): { name: string; definitions: string | undefined }[] => {
    const out: { name: string; definitions: string | undefined }[] = [];
    editor.core.view.state.doc.forEach((node) =>
      out.push({ name: node.type.name, definitions: node.attrs.definitions as string | undefined }),
    );
    return out;
  };

  it("keeps a block that refers to a footnote, and the definition, as they are", () => {
    const { text, reasons } = open(several);
    // a paragraph, a list and a table that refer to a footnote, then a plain paragraph, then the two definitions
    expect([...reasons]).toEqual([
      [0, "reference"],
      [1, "reference"],
      [2, "reference"],
      [4, "kind"],
      [5, "kind"],
    ]);
    expect(editor.text()).toBe(text);
  });

  it("draws each block that refers to footnotes with the definitions it uses, and no other", () => {
    open(several);
    const [paragraph, list, table, plain, one, named] = blocks();
    // in the order the block uses them (the renderer shows a marker, and none of the text)
    expect(paragraph.definitions).toBe(`${NOTE_1}\n\n${NOTE_NAMED}`);
    expect(list.definitions).toBe(NOTE_NAMED);
    expect(table.definitions).toBe(NOTE_1);
    expect(plain.name).toBe("paragraph");
    // a definition is a block of its own, drawn from its own source
    expect([one.name, one.definitions]).toEqual(["tm_locked", ""]);
    expect([named.name, named.definitions]).toEqual(["tm_locked", ""]);
  });

  it("finds the definitions wherever the file has them", () => {
    const { text } = open(between);
    expect(blocks().map((b) => b.name)).toEqual([
      "paragraph",
      "tm_locked",
      "tm_locked",
      "tm_locked",
      "paragraph",
    ]);
    expect(blocks()[2].definitions).toBe(
      "[^early]: Defined before it is used.\n\n[^late]: Defined after.",
    );
    expect(editor.text()).toBe(text);
  });

  it("writes every footnote block with its own bytes when a paragraph next to them is edited", () => {
    const { text } = open(several);
    typeAtEnd(3, "X");
    expect(editor.text()).toBe(text.replace("no notes.", "no notes.X"));
    const second = open(between).text;
    typeAtEnd(0, "X");
    typeAtEnd(4, "Y");
    expect(editor.text()).toBe(second.replace("Intro.", "Intro.X").replace("Outro.", "Outro.Y"));
  });

  it("keeps the definitions through an edit anywhere", () => {
    for (const name of [several, between]) {
      const { text } = open(name);
      const before = definitionsOf(text);
      expect(before.length).toBeGreaterThan(0);
      const count = editor.core.view.state.doc.childCount;
      for (let block = 0; block < count; block++) {
        editor.restart();
        if (endOfText(editor.core.view.state.doc, block) === null) continue;
        if (editor.core.view.state.doc.child(block).type.name === "tm_locked") continue;
        typeAtEnd(block, "X");
        expect(definitionsOf(editor.text()), `${name}: edit at the end of block ${block}`).toEqual(
          before,
        );
      }
    }
  });
});

// ---- S1: a block that changed kind ----------------------------------------------------------------------------

describe("a block that changes kind and sat one newline from its neighbour", () => {
  it("a heading made a paragraph does not merge into the paragraph above", () => {
    open("a paragraph, one newline, a heading");
    convert(1, "paragraph");
    const saved = editor.text();
    expect(saved).toBe("Intro text\n\nHeading\n\nAfter.\n");
    expect(topLevel(saved)).toEqual(["paragraph", "paragraph", "paragraph"]);
    expect(splitKinds(saved)).toEqual(["paragraph", "paragraph", "paragraph"]);
  });

  it("a code block made a paragraph does not merge into the paragraph above", () => {
    open("a paragraph, one newline, a fence");
    convert(1, "paragraph");
    const saved = editor.text();
    expect(saved).toBe("Para text\n\ncode\n\nAfter.\n");
    expect(topLevel(saved)).toEqual(["paragraph", "paragraph", "paragraph"]);
  });

  it("a heading made a paragraph does not become part of the list above", () => {
    open("a list, one newline, a heading");
    convert(1, "paragraph");
    const saved = editor.text();
    expect(saved).toBe("- a\n- b\n\nH\n");
    expect(topLevel(saved)).toEqual(["list:bullet", "paragraph"]);
  });

  it("a quote made a paragraph stays apart from the paragraph above", () => {
    open("a paragraph, one newline, a quote");
    convert(1, "quote");
    const saved = editor.text();
    expect(saved).toBe("Lead\n\nquoted\n\nAfter.\n");
    expect(topLevel(saved)).toEqual(["paragraph", "paragraph", "paragraph"]);
  });

  it("a paragraph made a heading, quote or list stays apart from the block below", () => {
    open("a paragraph, one newline, a quote");
    convert(0, "h2");
    expect(editor.text()).toBe("## Lead\n\n> quoted\n\nAfter.\n");
    editor.restart();
    convert(0, "quote");
    expect(editor.text()).toBe("> Lead\n\n> quoted\n\nAfter.\n");
    expect(topLevel(editor.text())).toEqual(["blockquote", "blockquote", "paragraph"]);
  });

  it("an edit that keeps the kind keeps the author's single newline", () => {
    open("a paragraph, one newline, a list");
    typeAtEnd(0, "X");
    expect(editor.text()).toBe("Intro:X\n- a\n- b\n\nOutro.\n");
    editor.restart();
    typeAtEnd(1, "X");
    expect(editor.text()).toBe("Intro:\n- a\n- bX\n\nOutro.\n");
  });
});

// ---- S5: a link is not extended by typing at its edge ---------------------------------------------------------

describe("typing at the edge of a link", () => {
  it("at the end of a paragraph ending in a link goes after the link", () => {
    const { text } = open("a link at the end and at the start of a paragraph");
    typeAtEnd(0, ".");
    expect(editor.text()).toBe(text.replace("See [docs](a.md)", "See [docs](a.md)."));
  });

  it("at the start of a paragraph beginning with a link goes before it", () => {
    const { text } = open("a link at the end and at the start of a paragraph");
    typeAtStart(1, "Y");
    expect(editor.text()).toBe(
      text.replace("<https://x.com> autolink", "Y<https://x.com> autolink"),
    );
  });

  it("inside the link text still extends it", () => {
    const { text } = open("a link at the end and at the start of a paragraph");
    const at = startOfText(editor.core.view.state.doc, 0) as number;
    typeAt(at + "See do".length, "X");
    expect(editor.text()).toBe(text.replace("[docs](a.md)", "[doXcs](a.md)"));
  });
});

// ---- S6: lists that are apart only by their marker ------------------------------------------------------------

describe("lists kept apart only by their marker", () => {
  it("editing the list with the other bullet keeps the two lists apart", () => {
    open("adjacent lists with different bullets");
    typeAtEnd(1, "X");
    const saved = editor.text();
    expect(saved).toBe("- a\n- b\n\n* c\n* dX\n");
    expect(topLevel(saved)).toEqual(["list:bullet", "list:bullet"]);
  });

  it("editing the first list does not change the second", () => {
    open("adjacent lists with different bullets");
    typeAtEnd(0, "X");
    expect(editor.text()).toBe("- a\n- bX\n\n* c\n* d\n");
  });

  it("editing the list with the other number delimiter keeps the numbering of both", () => {
    open("adjacent lists with different number delimiters");
    typeAtEnd(1, "X");
    const saved = editor.text();
    expect(saved).toBe("1. a\n2. b\n1) c\n2) dX\n");
    expect(topLevel(saved)).toEqual(["list:ordered", "list:ordered"]);
  });

  it("a paragraph made a list beside a list is a list of its own", () => {
    open("a paragraph after a list");
    convert(2, "bullet");
    const saved = editor.text();
    expect(saved).toBe("Para\n\n- a\n- b\n\n* Next\n");
    expect(topLevel(saved)).toEqual(["paragraph", "list:bullet", "list:bullet"]);
  });

  it("a paragraph made a list above a list is a list of its own", () => {
    open("a paragraph, one newline, a list");
    convert(0, "bullet");
    const saved = editor.text();
    expect(saved).toBe("* Intro:\n\n- a\n- b\n\nOutro.\n");
    expect(topLevel(saved)).toEqual(["list:bullet", "list:bullet", "paragraph"]);
  });

  it("a paragraph made a list does not take the indented paragraph below it into its last item", () => {
    const { text } = open("trailing spaces and hard breaks");
    // "   indented three spaces" stands under "trailing tab"; right under a list item it would be a second paragraph of it
    convert(1, "bullet");
    const saved = editor.text();
    expect(saved).toBe(
      text.replace("trailing tab\t", "- trailing tab").replace("   indented", "indented"),
    );
    expect(topLevel(saved)).toEqual(["paragraph", "list:bullet", "paragraph"]);
  });

  it("a list edited beside a list with the editor's own marker is not changed", () => {
    open("a paragraph after a list");
    typeAtEnd(1, "X");
    expect(editor.text()).toBe("Para\n\n- a\n- bX\n\nNext\n");
  });
});

// ---- S4: a link inside a locked block ------------------------------------------------------------------------------

describe("a link inside a locked block", () => {
  const html =
    '<p>See <a href="https://example.com/spec">the spec</a> and <a href="other.md">more</a>.</p>\n';

  function anchors(): HTMLAnchorElement[] {
    return Array.from(editor.core.view.dom.querySelectorAll("a"));
  }
  function click(target: Element, type: "click" | "auxclick" = "click"): MouseEvent {
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, button: 0 });
    target.dispatchEvent(event);
    return event;
  }

  it("is not followed by the window: the click is stopped and handed to the app", () => {
    followed.length = 0;
    editor.load({ map: buildBlockMap(html, mockParseBlocks(html)) });
    const [first, second] = anchors();
    expect(first).toBeDefined();
    expect(click(first).defaultPrevented).toBe(true);
    expect(click(second).defaultPrevented).toBe(true);
    expect(followed).toEqual(["https://example.com/spec", "other.md"]);
  });

  it("is not followed by the middle button either, and the app is not asked", () => {
    followed.length = 0;
    editor.load({ map: buildBlockMap(html, mockParseBlocks(html)) });
    expect(click(anchors()[0], "auxclick").defaultPrevented).toBe(true);
    expect(followed).toEqual([]);
  });

  it("leaves a click that is not on a link alone", () => {
    followed.length = 0;
    editor.load({ map: buildBlockMap(html, mockParseBlocks(html)) });
    const paragraph = editor.core.view.dom.querySelector(".tm-locked__body p") as Element;
    expect(click(paragraph).defaultPrevented).toBe(false);
    expect(followed).toEqual([]);
  });
});

// ---- N2: text the editor writes differently from how it was read -----------------------------------------------------

describe("what an edited block keeps of how its text was written", () => {
  it("keeps a no-break space as the &nbsp; it was written as, everywhere in the block, and not in code", () => {
    const text = "a&nbsp;b and `c&nbsp;d` and **e&nbsp;f** end\n";
    editor.load({ map: buildBlockMap(text, mockParseBlocks(text)) });
    expect(editor.status().dirty).toBe(false);
    typeAtEnd(0, "!");
    // inline code is code: its &nbsp; is the characters &nbsp;, which the editor reads and writes as they are
    expect(editor.text()).toBe("a&nbsp;b and `c&nbsp;d` and **e&nbsp;f** end!\n");
  });

  it("writes the other character references as the characters they stand for, which means the same", () => {
    const { text } = open("character references and bare urls");
    typeAtEnd(0, "!");
    expect(editor.text()).toBe(text.replace("&copy; sign", "\u00a9 sign!"));
    editor.restart();
    typeAtEnd(1, "!");
    expect(editor.text()).toBe(
      text.replace(
        "https://example.com/page now, ~x~ struck.",
        "<https://example.com/page> now, ~~x~~ struck.!",
      ),
    );
  });
});
