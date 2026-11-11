// @vitest-environment jsdom
import "./jsdomShims";
import { undo } from "@milkdown/kit/prose/history";
import { AllSelection, NodeSelection, TextSelection } from "@milkdown/kit/prose/state";
import { $prose } from "@milkdown/kit/utils";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import type { Segment } from "../../../api/generated/Segment";
import type { TextEdit } from "../session.svelte";
import { buildDoc, compose, type Built, type Composed } from "./compose";
import { createCore, type Core } from "./core";
import { applyEdits, blockOf, changeable, docRange, scopeIn, selectRange } from "./edits";
import { markAt, marksPlugin, setMarks } from "./marks";

let core: Core;
let built: Built;

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.append(host);
  core = await createCore({ host, plugins: [$prose(() => marksPlugin())] });
});

afterAll(async () => {
  await core.destroy();
});

/** Blank-line separated runs (test texts only), as in compose.test.ts. */
function segmentsOf(text: string): Segment[] {
  const lines = text.split("\n");
  const out: Segment[] = [];
  let start = -1;
  lines.forEach((line, i) => {
    const filled = line.trim() !== "";
    if (filled && start < 0) start = i;
    if (!filled && start >= 0) {
      out.push({ lineStart: start + 1, lineEnd: i, kind: "paragraph", lock: null });
      start = -1;
    }
  });
  if (start >= 0)
    out.push({ lineStart: start + 1, lineEnd: lines.length, kind: "paragraph", lock: null });
  return out;
}

async function load(text: string): Promise<void> {
  built = await buildDoc(core, text, segmentsOf(text));
  core.setDoc(built.doc);
}

const now = (): Composed => compose(core, built, core.view.state.doc);

/** A word fix: `word` (first after `after`) becomes `insert`. */
function fix(text: string, word: string, insert: string, after = 0): TextEdit {
  const from = text.indexOf(word, after);
  if (from < 0) throw new Error(`no ${word}`);
  return { from, to: from + word.length, insert, expect: word };
}

/** Document position of `word` in the editor. */
function posOf(word: string): number {
  let found = -1;
  core.view.state.doc.descendants((node, pos) => {
    if (found >= 0 || !node.isText) return;
    const at = (node.text ?? "").indexOf(word);
    if (at >= 0) found = pos + at;
  });
  if (found < 0) throw new Error(`no ${word}`);
  return found;
}

function select(from: number, to = from): void {
  const view = core.view;
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, from, to)));
}

// odd but kept styles: `_` emphasis, `__` strong, `*` bullets, trailing spaces, extra blank lines
const DOC =
  "# Teh _title_\n\nSecond paragrap with *emph* here.\n\n\n* item _one_  \n* item two\n\nLast __bold__ line.\n";
const BLOCK_2 = "Second paragrap with *emph* here.";

describe("Edit-mode ranges", () => {
  it("each top-level block's range reads its own text in the composed text", async () => {
    await load(DOC);
    const composed = now();
    expect(composed.text).toBe(DOC);
    const texts = composed.ranges.map((r) => (r ? composed.text.slice(r.from, r.to) : null));
    expect(texts).toEqual([
      "# Teh _title_",
      BLOCK_2,
      "* item _one_  \n* item two",
      "Last __bold__ line.",
    ]);
    expect(blockOf(composed, DOC.indexOf("paragrap"), DOC.indexOf("here"))).toBe(1);
    // the blank lines between two blocks belong to none
    expect(blockOf(composed, DOC.indexOf("\n\n\n"), DOC.indexOf("\n\n\n") + 1)).toBeNull();
  });
});

describe("what a correction works on in the Edit-mode editor", () => {
  it("paragraph: the whole top-level block at the caret", async () => {
    await load(DOC);
    select(posOf("paragrap") + 2);
    const start = DOC.indexOf(BLOCK_2);
    expect(scopeIn(core.view.state, now(), "paragraph")).toEqual({
      from: start,
      to: start + BLOCK_2.length,
    });
    select(posOf("two"));
    const list = DOC.indexOf("* item _one_");
    expect(scopeIn(core.view.state, now(), "paragraph")).toEqual({
      from: list,
      to: DOC.indexOf("two") + 3,
    });
  });

  it("selection: the whole blocks it touches; none selected = nothing", async () => {
    await load(DOC);
    select(posOf("paragrap"));
    expect(scopeIn(core.view.state, now(), "selection")).toBeNull();
    select(posOf("with"), posOf("one"));
    expect(scopeIn(core.view.state, now(), "selection")).toEqual({
      from: DOC.indexOf(BLOCK_2),
      to: DOC.indexOf("two") + 3,
    });
    const view = core.view;
    view.dispatch(view.state.tr.setSelection(new AllSelection(view.state.doc)));
    expect(scopeIn(view.state, now(), "selection")).toEqual({
      from: 0,
      to: DOC.length - 1,
    });
  });

  it("document: the whole text", async () => {
    await load(DOC);
    expect(scopeIn(core.view.state, now(), "document")).toEqual({ from: 0, to: DOC.length });
  });
});

describe("what an edit could change in the Edit-mode editor", () => {
  const TEXT = "One here.\n\nTwo there.\n\nMath $x_1\ny$ here.\n";

  it("a range inside one block the editor writes; not between blocks, across two or in a locked block", async () => {
    await load(TEXT);
    const composed = now();
    expect(changeable(core, composed, wordAt(TEXT, "One here."))).toBe(true);
    expect(changeable(core, composed, wordAt(TEXT, "there"))).toBe(true);
    const gap = TEXT.indexOf("\n\n");
    expect(changeable(core, composed, { from: gap, to: gap + 1 })).toBe(false);
    const across = { from: TEXT.indexOf("here."), to: TEXT.indexOf("Two") + 3 };
    expect(changeable(core, composed, across)).toBe(false);
    expect(changeable(core, composed, wordAt(TEXT, "Math"))).toBe(false);
    expect(changeable(core, composed, wordAt(TEXT, "Math $x_1\ny$ here."))).toBe(false);
  });

  it("with the text to put there: what the edit itself would meet, and nothing is changed", async () => {
    const text = "A list:\n\nSee r here.\n\n[r]: http://x\n";
    await load(text);
    const before = core.view.state.doc;
    const taken = fix(text, "See", "Look");
    // a link by reference | a paragraph turned into a heading: its block would read as something else
    const refused = [fix(text, "r here", "[r] here"), fix(text, "A list:", "# A list:")];
    expect(changeable(core, now(), taken, taken.insert)).toBe(true);
    for (const edit of refused) {
      // the place alone can be changed; this text cannot go there
      expect(changeable(core, now(), edit)).toBe(true);
      expect(changeable(core, now(), edit, edit.insert)).toBe(false);
      expect(applyEdits(core, now(), [edit])).toBe(false);
    }
    expect(core.view.state.doc).toBe(before);
    expect(applyEdits(core, now(), [taken])).toBe(true);
  });

  it("a locked block that is selected is a scope like any other: what may change in it is asked apart", async () => {
    await load(TEXT);
    const view = core.view;
    const at = view.state.doc.child(0).nodeSize + view.state.doc.child(1).nodeSize;
    view.dispatch(view.state.tr.setSelection(NodeSelection.create(view.state.doc, at)));
    const block = { from: TEXT.indexOf("Math"), to: TEXT.length - 1 };
    expect(scopeIn(view.state, now(), "paragraph")).toEqual(block);
    expect(scopeIn(view.state, now(), "selection")).toEqual(block);
    expect(changeable(core, now(), block)).toBe(false);
  });
});

describe("checked edits in the Edit-mode editor", () => {
  it("a word fix rewrites only its block: every other block keeps its bytes", async () => {
    await load(DOC);
    const before = core.view.state.doc;
    expect(applyEdits(core, now(), [fix(DOC, "paragrap", "paragraph")])).toBe(true);
    const after = now();
    expect(after.text).toBe(DOC.replace("paragrap ", "paragraph "));
    expect(after.changed).toBe(1);
    // the other nodes are the same objects
    [0, 2, 3].forEach((i) => expect(core.view.state.doc.child(i)).toBe(before.child(i)));
  });

  it("the touched block is written by the editor, as typing would (its `_` emphasis stays)", async () => {
    const text = "Kept _one_.\n\nFixd _two_.\n\nKept _three_.\n";
    await load(text);
    expect(applyEdits(core, now(), [fix(text, "Fixd", "Fixed")])).toBe(true);
    expect(now().text).toBe("Kept _one_.\n\nFixed _two_.\n\nKept _three_.\n");
  });

  it("edits in two blocks go in together and come out with one undo", async () => {
    await load(DOC);
    const edits = [fix(DOC, "Teh", "The"), fix(DOC, "paragrap", "paragraph")];
    expect(applyEdits(core, now(), edits)).toBe(true);
    expect(now().text).toBe(DOC.replace("Teh", "The").replace("paragrap ", "paragraph "));
    const view = core.view;
    expect(undo(view.state, view.dispatch)).toBe(true);
    expect(now().text).toBe(DOC);
    expect(now().changed).toBe(0);
  });

  it("typing right after a fix is an undo step of its own", async () => {
    await load(DOC);
    expect(applyEdits(core, now(), [fix(DOC, "paragrap", "paragraph")])).toBe(true);
    const view = core.view;
    const at = posOf("here");
    view.dispatch(view.state.tr.insertText("x", at));
    expect(undo(view.state, view.dispatch)).toBe(true);
    expect(now().text).toBe(DOC.replace("paragrap ", "paragraph "));
  });

  it("nothing changes when a range no longer reads what was expected", async () => {
    await load(DOC);
    const before = core.view.state.doc;
    const stale = { ...fix(DOC, "paragrap", "paragraph"), expect: "paragrab" };
    expect(applyEdits(core, now(), [fix(DOC, "Teh", "The"), stale])).toBe(false);
    expect(core.view.state.doc).toBe(before);
  });

  it("refuses an edit across two blocks, between blocks, or in a locked block", async () => {
    const text = "One here.\n\nTwo there.\n\nMath $x_1\ny$ here.\n";
    await load(text);
    const before = core.view.state.doc;
    const across = text.indexOf("here.");
    const end = text.indexOf("Two") + 3;
    const span = text.slice(across, end);
    expect(applyEdits(core, now(), [{ from: across, to: end, insert: "x", expect: span }])).toBe(
      false,
    );
    const gap = text.indexOf("\n\n");
    expect(applyEdits(core, now(), [{ from: gap, to: gap + 1, insert: " ", expect: "\n" }])).toBe(
      false,
    );
    expect(applyEdits(core, now(), [fix(text, "Math", "Maths")])).toBe(false);
    expect(core.view.state.doc).toBe(before);
  });

  it("refuses a fix that would make its block read as something else", async () => {
    const text = "A list:\n\nSee r here.\n\n[r]: http://x\n";
    await load(text);
    const before = core.view.state.doc;
    // `[r]` would use the definition below: a link by reference, never made by a fix
    expect(applyEdits(core, now(), [fix(text, "r here", "[r] here")])).toBe(false);
    // a paragraph turned into a heading
    expect(applyEdits(core, now(), [fix(text, "A list:", "# A list:")])).toBe(false);
    expect(core.view.state.doc).toBe(before);
  });
});

/** Composed-text range of the `nth` (0-based) `word` in `text`. */
function wordAt(text: string, word: string, nth = 0): { from: number; to: number } {
  let from = -1;
  for (let i = 0; i <= nth; i++) from = text.indexOf(word, from + 1);
  if (from < 0) throw new Error(`no ${word}`);
  return { from, to: from + word.length };
}

const docText = (r: { from: number; to: number } | null): string | null =>
  r === null ? null : core.view.state.doc.textBetween(r.from, r.to);

// "teh" five times: bold, link text, link target (not shown), escaped emphasis, plain
const MIXED =
  "Intro line.\n\nSome **bold teh** and [teh link](teh.md) with \\*teh\\* here teh.\n\n- teh item\n";

describe("text offsets -> document positions in the Edit-mode editor", () => {
  it("finds each word inside its block through bold, links and escapes", async () => {
    await load(MIXED);
    const composed = now();
    const doc = core.view.state.doc;
    const words = [0, 1, 3, 4].map((nth) => docRange(doc, composed, wordAt(MIXED, "teh", nth)));
    expect(words.map(docText)).toEqual(["teh", "teh", "teh", "teh"]);
    // each one its own place, in order: bold, link text, escaped, plain
    const froms = words.map((r) => r?.from ?? -1);
    expect([...froms].sort((a, b) => a - b)).toEqual(froms);
    expect(new Set(froms).size).toBe(4);
    const bold = words[0]?.from ?? -1;
    expect(
      doc
        .resolve(bold + 1)
        .marks()
        .map((m) => m.type.name),
    ).toContain("strong");
    const linked = words[1]?.from ?? -1;
    expect(
      doc
        .resolve(linked + 1)
        .marks()
        .map((m) => m.type.name),
    ).toContain("link");
    // in a list item's paragraph
    expect(docText(docRange(doc, composed, wordAt(MIXED, "teh", 5)))).toBe("teh");
    expect(docText(docRange(doc, composed, wordAt(MIXED, "Intro")))).toBe("Intro");
  });

  it("an insertion point sits right after the text before it", async () => {
    await load(MIXED);
    const at = MIXED.indexOf(" here");
    const found = docRange(core.view.state.doc, now(), { from: at, to: at });
    expect(found).not.toBeNull();
    const pos = found?.from ?? -1;
    expect(found?.to).toBe(pos);
    expect(core.view.state.doc.textBetween(pos - 4, pos + 5)).toBe("teh* here");
  });

  it("nothing for a range between blocks, in a locked block, or with markup inside", async () => {
    const text = "One **two** three.\n\nMath $x_1\ny$ here.\n";
    await load(text);
    const composed = now();
    const doc = core.view.state.doc;
    const gap = text.indexOf("\n\n");
    expect(docRange(doc, composed, { from: gap, to: gap + 1 })).toBeNull();
    expect(docRange(doc, composed, wordAt(text, "Math"))).toBeNull();
    expect(docRange(doc, composed, wordAt(text, "**two**"))).toBeNull();
  });

  it("reveal selects the exact word", async () => {
    await load(MIXED);
    expect(selectRange(core.view, now(), wordAt(MIXED, "teh", 3))).toBe(true);
    const { from, to } = core.view.state.selection;
    expect(core.view.state.doc.textBetween(from, to)).toBe("teh");
    expect(docRange(core.view.state.doc, now(), wordAt(MIXED, "teh", 3))).toEqual({ from, to });
    // not found (a locked block): nothing selected, the caller scrolls the block instead
    const text = "Math $x_1\ny$ here.\n";
    await load(text);
    const before = core.view.state.selection;
    expect(selectRange(core.view, now(), wordAt(text, "here"))).toBe(false);
    expect(core.view.state.selection.eq(before)).toBe(true);
  });
});

const marks = (): string[] =>
  Array.from(core.view.dom.querySelectorAll(".correction-mark")).map((el) =>
    el.classList.contains("correction-mark--insert")
      ? `^${el.getAttribute("data-correction-mark")}`
      : `${el.textContent}${el.classList.contains("correction-mark--current") ? "*" : ""}`,
  );

describe("correction marks in the Edit-mode editor", () => {
  it("one decoration per found range, the current one distinct; an empty list clears them", async () => {
    await load(MIXED);
    const at = MIXED.indexOf(" here");
    setMarks(
      core.view,
      now(),
      [
        wordAt(MIXED, "teh", 0),
        wordAt(MIXED, "teh", 3),
        { from: at, to: at },
        wordAt(MIXED, "Intro"),
      ],
      1,
      () => {},
    );
    expect(marks()).toEqual(["Intro", "teh", "teh*", "^2"]);
    expect(
      core.view.dom.querySelector(".correction-mark--insert")?.getAttribute("aria-hidden"),
    ).toBe("true");
    setMarks(core.view, now(), [], null, () => {});
    expect(marks()).toEqual([]);
  });

  it("marks follow typing before them and are not part of the undo history", async () => {
    await load(MIXED);
    const view = core.view;
    setMarks(view, now(), [wordAt(MIXED, "teh", 4)], 0, () => {});
    view.dispatch(view.state.tr.insertText("Very ", posOf("Some")));
    expect(marks()).toEqual(["teh*"]);
    expect(undo(view.state, view.dispatch)).toBe(true);
    expect(now().text).toBe(MIXED);
    expect(marks()).toEqual(["teh*"]);
  });

  it("a click on a mark picks it (the caret is left alone); the mark under the caret", async () => {
    await load(MIXED);
    const picked: number[] = [];
    const view = core.view;
    setMarks(view, now(), [wordAt(MIXED, "teh", 0), wordAt(MIXED, "teh", 4)], 0, (i) =>
      picked.push(i),
    );
    const caret = posOf("Intro") + 2;
    select(caret);
    const second = view.dom.querySelectorAll(".correction-mark")[1];
    second?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
    expect(picked).toEqual([1]);
    expect(view.state.selection.from).toBe(caret);
    expect(markAt(view.state)).toBeNull();
    const plain = docRange(view.state.doc, now(), wordAt(MIXED, "teh", 4));
    select((plain?.from ?? 0) + 1);
    expect(markAt(view.state)).toBe(1);
  });
});

describe("inline formulas in correction", () => {
  const TEXT = "Teh energy $teh=mc^2$ of teh mass.\n";

  it("marks the prose around a formula, never its TeX", async () => {
    await load(TEXT);
    // the first "teh" is the formula's: it is never taken for the word of the same text after it
    setMarks(core.view, now(), [wordAt(TEXT, "teh", 0), wordAt(TEXT, "teh", 1)], null, () => {});
    expect(marks()).toEqual(["teh"]);
    const doc = core.view.state.doc;
    expect(docRange(doc, now(), wordAt(TEXT, "teh", 0))).toBeNull();
    expect(docText(docRange(doc, now(), wordAt(TEXT, "teh", 1)))).toBe("teh");
    expect(docRange(doc, now(), wordAt(TEXT, "$teh=mc^2$"))).toBeNull();
    expect(docText(docRange(doc, now(), wordAt(TEXT, "energy")))).toBe("energy");
  });

  it("a fix in the prose goes in and keeps the formula's bytes; one in the TeX is refused", async () => {
    await load(TEXT);
    expect(applyEdits(core, now(), [fix(TEXT, "teh", "the", 25)])).toBe(true);
    const fixed = TEXT.replace("of teh", "of the");
    expect(now().text).toBe(fixed);
    const before = core.view.state.doc;
    expect(applyEdits(core, now(), [fix(fixed, "teh", "the", 10)])).toBe(false);
    expect(applyEdits(core, now(), [fix(fixed, "$teh=mc^2$", "teh")])).toBe(false);
    expect(core.view.state.doc).toBe(before);
  });
});

describe("references in correction", () => {
  const TEXT =
    "Teh [teh site][s] and teh note[^teh].\n\n[s]: https://example.com\n\n[^teh]: Note.\n";

  it("marks the prose around a reference, never its source", async () => {
    await load(TEXT);
    const words = [0, 1, 2, 3].map((n) => wordAt(TEXT, "teh", n));
    setMarks(core.view, now(), words, null, () => {});
    expect(marks()).toEqual(["teh"]);
    const doc = core.view.state.doc;
    expect(docRange(doc, now(), words[0] ?? wordAt(TEXT, "teh"))).toBeNull();
    expect(docRange(doc, now(), words[2] ?? wordAt(TEXT, "teh"))).toBeNull();
    expect(docText(docRange(doc, now(), words[1] ?? wordAt(TEXT, "teh")))).toBe("teh");
  });

  it("a fix in the prose goes in and keeps each reference's bytes; one in a reference is refused", async () => {
    await load(TEXT);
    expect(applyEdits(core, now(), [fix(TEXT, "teh", "the", 18)])).toBe(true);
    const fixed = TEXT.replace("and teh", "and the");
    expect(now().text).toBe(fixed);
    const before = core.view.state.doc;
    expect(applyEdits(core, now(), [fix(fixed, "teh", "the", 4)])).toBe(false);
    expect(applyEdits(core, now(), [fix(fixed, "[^teh]", "[^the]")])).toBe(false);
    expect(core.view.state.doc).toBe(before);
  });
});

describe("table cells in correction", () => {
  // columns padded by hand with an escaped pipe, then a table written tight: neither is how the editor writes one
  const TABLE =
    "| Name    | Note      |\n| :------ | --------: |\n| teh cat | a \\| b    |\n| other   | recieve   |";
  const TEXT = `Kept _one_.\n\n${TABLE}\n\n* kept  \n\n|Nme|Note|\n|:-:|---|\n|teh|x \\| y|\n`;

  it("paragraph: the caret in a cell = the whole table (a top-level block)", async () => {
    await load(TEXT);
    select(posOf("cat"));
    const from = TEXT.indexOf(TABLE);
    expect(scopeIn(core.view.state, now(), "paragraph")).toEqual({
      from,
      to: from + TABLE.length,
    });
  });

  it("a fix in a cell changes that cell's text alone: pipes, padding, alignment row, other cells and blocks stay", async () => {
    await load(TEXT);
    const before = core.view.state.doc;
    expect(docText(docRange(before, now(), wordAt(TEXT, "teh")))).toBe("teh");
    expect(applyEdits(core, now(), [fix(TEXT, "teh", "the")])).toBe(true);
    const after = now();
    expect(after.text).toBe(TEXT.replace("teh cat", "the cat"));
    expect(after.changed).toBe(1);
    // the other nodes are the same objects
    [0, 2, 3].forEach((i) => expect(core.view.state.doc.child(i)).toBe(before.child(i)));
  });

  it("a cell made longer | shorter leaves the columns as written: nothing is padded again", async () => {
    await load(TEXT);
    expect(applyEdits(core, now(), [fix(TEXT, "other", "another")])).toBe(true);
    expect(now().text).toBe(TEXT.replace("| other   |", "| another   |"));
    await load(TEXT);
    expect(applyEdits(core, now(), [fix(TEXT, "teh cat", "cat")])).toBe(true);
    expect(now().text).toBe(TEXT.replace("| teh cat |", "| cat |"));
  });

  it("several cells changing width: only spaces that pad the table's cells may move; every other block stays", async () => {
    await load(TEXT);
    const edits = [fix(TEXT, "teh cat", "the big cat"), fix(TEXT, "recieve", "got")];
    expect(applyEdits(core, now(), edits)).toBe(true);
    const after = now();
    // the editor pads the columns anew; where that moves a pipe, spaces of it may stay (the header row's too)
    const squeezed = (text: string): string => text.replace(/ +/g, " ");
    const replaced = TEXT.replace("teh cat", "the big cat").replace("recieve", "got");
    expect(squeezed(after.text)).toBe(squeezed(replaced));
    expect(after.text).toContain("\n| :------ | --------: |\n");
    const from = TEXT.indexOf(TABLE);
    expect(after.text.slice(0, from)).toBe(TEXT.slice(0, from));
    expect(after.text.endsWith(TEXT.slice(from + TABLE.length))).toBe(true);
    expect(after.changed).toBe(1);
  });

  it("refuses a fix that would add a cell | a line to the row", async () => {
    await load(TEXT);
    const before = core.view.state.doc;
    expect(applyEdits(core, now(), [fix(TEXT, "other", "ot | her")])).toBe(false);
    expect(applyEdits(core, now(), [fix(TEXT, "other", "ot\nher")])).toBe(false);
    expect(core.view.state.doc).toBe(before);
  });

  it("a header cell and a body cell of a table written tight; one undo takes both back", async () => {
    await load(TEXT);
    const tight = TEXT.indexOf("|Nme|");
    const edits = [fix(TEXT, "Nme", "Name"), fix(TEXT, "teh", "the", tight)];
    expect(applyEdits(core, now(), edits)).toBe(true);
    expect(now().text).toBe(TEXT.replace("|Nme|", "|Name|").replace("|teh|", "|the|"));
    const view = core.view;
    expect(undo(view.state, view.dispatch)).toBe(true);
    expect(now().text).toBe(TEXT);
    expect(now().changed).toBe(0);
  });
});

describe("a piece after the same text written as prose", () => {
  it.each([
    ["a formula", "Teh \\$teh$ and $teh$ teh.\n", "teh"],
    ["a reference", "*Teh \\[s]* and [s] teh.\n\n[s]: https://example.com\n", "s]"],
  ])("%s: the prose is found, never the piece's source", async (_, text, word) => {
    await load(text);
    const doc = core.view.state.doc;
    // the first one is escaped prose, the second one inside the piece's own source
    expect(docText(docRange(doc, now(), wordAt(text, word, 0)))).toBe(word);
    expect(docRange(doc, now(), wordAt(text, word, 1))).toBeNull();
  });
});
