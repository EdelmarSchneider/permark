import { EditorSelection, EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { toggleBold } from "./bold";

function run(doc: string, ranges: [number, number][]): { doc: string; sel: [number, number][] } {
  const state = EditorState.create({
    doc,
    selection: EditorSelection.create(ranges.map(([a, b]) => EditorSelection.range(a, b))),
    extensions: EditorState.allowMultipleSelections.of(true),
  });
  const next = state.update(toggleBold(state)).state;
  return {
    doc: next.doc.toString(),
    sel: next.selection.ranges.map((r) => [r.from, r.to]),
  };
}

describe("Ctrl+B in the Markdown source", () => {
  it("wraps the selection, keeping it on the word", () => {
    expect(run("say hello now", [[4, 9]])).toEqual({ doc: "say **hello** now", sel: [[6, 11]] });
  });

  it("an empty selection gets a pair with the caret inside", () => {
    expect(run("ab", [[1, 1]])).toEqual({ doc: "a****b", sel: [[3, 3]] });
  });

  it("a selection with ** around it loses them (bold off); the rest stays as it was", () => {
    expect(run("say **hello** now\n", [[6, 11]])).toEqual({
      doc: "say hello now\n",
      sel: [[4, 9]],
    });
  });

  it("several selections, Unicode untouched around them", () => {
    const doc = "olá mundo\n日本語 ok";
    const out = run(doc, [
      [0, 3],
      [10, 13],
    ]);
    expect(out.doc).toBe("**olá** mundo\n**日本語** ok");
  });

  it("at the very start and end of the document", () => {
    expect(run("x", [[0, 1]]).doc).toBe("**x**");
    expect(run("", [[0, 0]]).doc).toBe("****");
  });
});
