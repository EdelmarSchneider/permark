import { history, undo } from "@codemirror/commands";
import { EditorSelection, EditorState, type Extension } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { languageFor } from "./cm";
import { editsSpec, scopeIn } from "./edits";

function markdownLanguage(): Extension {
  const language = languageFor("note.md", true);
  if (language instanceof Promise) throw new Error("Markdown loads at once");
  return language;
}

function state(doc: string, anchor = 0, head = anchor): EditorState {
  return EditorState.create({
    doc,
    selection: EditorSelection.single(anchor, head),
    extensions: [history(), markdownLanguage()],
  });
}

const DOC =
  "---\ntitle: x\n---\n\n# Teh title\n\nFirst paragrap\nstill first.\n\n- item won\n\n```\ncode\n```\n";

describe("what a correction works on in the Source editor", () => {
  it("selection: the selected range; none selected = nothing", () => {
    const at = DOC.indexOf("paragrap");
    expect(scopeIn(state(DOC, at, at + 8), "selection")).toEqual({ from: at, to: at + 8 });
    expect(scopeIn(state(DOC, at + 8, at), "selection")).toEqual({ from: at, to: at + 8 });
    expect(scopeIn(state(DOC, at), "selection")).toBeNull();
  });

  it("paragraph: the Markdown block around the caret (paragraph, heading, list item text)", () => {
    const para = DOC.indexOf("First");
    const paraEnd = DOC.indexOf("first.") + "first.".length;
    expect(scopeIn(state(DOC, para + 3), "paragraph")).toEqual({ from: para, to: paraEnd });
    // the caret right after the last character still belongs to the paragraph
    expect(scopeIn(state(DOC, paraEnd), "paragraph")).toEqual({ from: para, to: paraEnd });
    const heading = DOC.indexOf("# Teh");
    expect(scopeIn(state(DOC, heading + 4), "paragraph")).toEqual({
      from: heading,
      to: heading + "# Teh title".length,
    });
    const item = DOC.indexOf("item won");
    expect(scopeIn(state(DOC, item + 2), "paragraph")).toEqual({ from: item, to: item + 8 });
    // a task item's text is a `Task`, no `Paragraph`
    const tasks = "- [ ] Teh task\n";
    expect(scopeIn(state(tasks, tasks.indexOf("task")), "paragraph")).toEqual({
      from: 2,
      to: tasks.length - 1,
    });
  });

  it("paragraph: in a table, the cell at the caret; its pipes, padding and alignment row = nothing", () => {
    const table = "| Teh head | b |\n|:---------|--:|\n|  teh cell   |   |\n";
    const head = table.indexOf("Teh head");
    expect(scopeIn(state(table, head + 2), "paragraph")).toEqual({ from: head, to: head + 8 });
    const cell = table.indexOf("teh cell");
    expect(scopeIn(state(table, cell), "paragraph")).toEqual({ from: cell, to: cell + 8 });
    // the caret right after the last character still belongs to the cell
    expect(scopeIn(state(table, cell + 8), "paragraph")).toEqual({ from: cell, to: cell + 8 });
    expect(scopeIn(state(table, cell + 10), "paragraph")).toBeNull();
    expect(scopeIn(state(table, table.indexOf("|:--") + 3), "paragraph")).toBeNull();
    expect(scopeIn(state(table, table.lastIndexOf("|") - 2), "paragraph")).toBeNull();
  });

  it("paragraph: no prose block at the caret (blank line, code, front matter) = nothing", () => {
    expect(scopeIn(state(DOC, DOC.indexOf("\n\nFirst") + 1), "paragraph")).toBeNull();
    expect(scopeIn(state(DOC, DOC.indexOf("code") + 1), "paragraph")).toBeNull();
    expect(scopeIn(state(DOC, DOC.indexOf("title:") + 2), "paragraph")).toBeNull();
  });

  it("document: all of it", () => {
    expect(scopeIn(state(DOC), "document")).toEqual({ from: 0, to: DOC.length });
  });
});

describe("edits a correction applies in the Source editor", () => {
  // the edit text (LF; the save puts the file's own line endings back)
  const doc = "Teh cat  \nsat on teh mat. ☕\n";

  function apply(s: EditorState, edits: Parameters<typeof editsSpec>[1]): EditorState | null {
    const spec = editsSpec(s, edits);
    return spec === null ? null : s.update(spec).state;
  }

  it("puts in only the replacement text: every other byte stays", () => {
    const next = apply(state(doc), [
      { from: 0, to: 3, insert: "The", expect: "Teh" },
      { from: 17, to: 20, insert: "the", expect: "teh" },
    ]);
    expect(next?.doc.toString()).toBe("The cat  \nsat on the mat. ☕\n");
  });

  it("a fix in a table cell changes its text alone: pipes, padding, alignment row and other cells stay", () => {
    const table =
      "| Name    | Note    |\n| :------ | ------: |\n| teh cat | a \\| b  |\n| other   | recieve |\n";
    const cat = table.indexOf("teh");
    const other = table.indexOf("other");
    const next = apply(state(table), [
      { from: cat, to: cat + 3, insert: "the", expect: "teh" },
      { from: other, to: other + 5, insert: "another", expect: "other" },
    ]);
    // a longer cell moves its own row's pipes on; nothing is padded again
    expect(next?.doc.toString()).toBe(
      "| Name    | Note    |\n| :------ | ------: |\n| the cat | a \\| b  |\n| another   | recieve |\n",
    );
  });

  it("all or nothing: one range whose text moved on = no change at all", () => {
    const s = state(doc);
    expect(
      editsSpec(s, [
        { from: 0, to: 3, insert: "The", expect: "Teh" },
        { from: 17, to: 20, insert: "the", expect: "tha" },
      ]),
    ).toBeNull();
    expect(s.doc.toString()).toBe(doc);
  });

  it("ranges outside the text, backwards or overlapping are refused", () => {
    const s = state(doc);
    expect(editsSpec(s, [{ from: 21, to: 99, insert: "", expect: "" }])).toBeNull();
    expect(editsSpec(s, [{ from: 3, to: 0, insert: "", expect: "" }])).toBeNull();
    expect(
      editsSpec(s, [
        { from: 0, to: 3, insert: "The", expect: "Teh" },
        { from: 2, to: 5, insert: "x", expect: "h c" },
      ]),
    ).toBeNull();
    expect(editsSpec(s, [])).toBeNull();
  });

  it("one undo step takes all the edits back", () => {
    let s = state(doc);
    const next = apply(s, [
      { from: 17, to: 20, insert: "the", expect: "teh" },
      { from: 0, to: 3, insert: "The", expect: "Teh" },
    ]);
    if (next === null) throw new Error("not applied");
    s = next;
    expect(s.doc.toString()).toBe("The cat  \nsat on the mat. ☕\n");
    undo({
      state: s,
      dispatch: (tr) => {
        s = tr.state;
      },
    });
    expect(s.doc.toString()).toBe(doc);
  });
});
