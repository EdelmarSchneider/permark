// @vitest-environment jsdom
// Correction marks in Source (CodeMirror): every range marked, the current one distinct, an insertion a caret-width
// widget; marks follow edits, go on an empty list; a click on one picks it (the caret is left alone); the mark under
// the caret.

import { EditorSelection } from "@codemirror/state";
import { afterEach, describe, expect, it } from "vitest";
import { createSource, type SourceHost, type SourceView } from "./cm";

const host: SourceHost = {
  changed: () => {},
  selected: () => {},
  save: () => {},
  modeKey: () => {},
  pictures: () => {},
  fence: () => {},
  docPath: () => "a.md",
  linkTargets: async () => [],
  headings: async () => [],
};

let made: SourceView | null = null;

function source(text: string): SourceView {
  const parent = document.createElement("div");
  document.body.append(parent);
  made = createSource({ parent, text, path: "a.md", markdown: true, host });
  return made;
}

afterEach(() => {
  made?.destroy();
  made = null;
  document.body.replaceChildren();
});

const TEXT = "I has teh cat\n\nShe recieve mail";
const range = (word: string) => {
  const from = TEXT.indexOf(word);
  return { from, to: from + word.length };
};

const marked = (view: SourceView): string[] =>
  Array.from(view.view.contentDOM.querySelectorAll(".correction-mark")).map((el) =>
    el.classList.contains("correction-mark--insert")
      ? `^${el.getAttribute("data-correction-mark")}`
      : `${el.textContent}${el.classList.contains("correction-mark--current") ? "*" : ""}`,
  );

describe("correction marks in Source", () => {
  it("marks each range, the current one distinct; an empty list clears them", () => {
    const view = source(TEXT);
    view.marks([range("teh"), range("recieve")], 1, () => {});
    expect(marked(view)).toEqual(["teh", "recieve*"]);
    view.marks([], null, () => {});
    expect(marked(view)).toEqual([]);
  });

  it("an insertion (empty range) gets a caret-width mark, hidden from screen readers", () => {
    const view = source(TEXT);
    const at = TEXT.indexOf(" teh");
    view.marks([{ from: at, to: at }], 0, () => {});
    expect(marked(view)).toEqual(["^0"]);
    const widget = view.view.contentDOM.querySelector(".correction-mark--insert");
    expect(widget?.getAttribute("aria-hidden")).toBe("true");
  });

  it("marks follow an edit before them", () => {
    const view = source(TEXT);
    view.marks([range("recieve")], 0, () => {});
    view.view.dispatch({ changes: { from: 0, insert: "Well, " } });
    expect(marked(view)).toEqual(["recieve*"]);
    const el = view.view.contentDOM.querySelector(".correction-mark");
    expect(el && view.view.posAtDOM(el)).toBe(range("recieve").from + "Well, ".length);
  });

  it("a click on a mark picks it; the caret stays where the person put it", () => {
    const view = source(TEXT);
    const picked: number[] = [];
    view.marks([range("teh"), range("recieve")], 0, (index) => picked.push(index));
    const caret = range("recieve").from + 2;
    view.view.dispatch({ selection: EditorSelection.cursor(caret) });
    const second = view.view.contentDOM.querySelectorAll(".correction-mark")[1];
    second?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
    expect(picked).toEqual([1]);
    expect(view.view.state.selection.main.head).toBe(caret);
    // a click elsewhere picks nothing
    view.view.contentDOM.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(picked).toEqual([1]);
  });

  it("the mark under the caret", () => {
    const view = source(TEXT);
    view.marks([range("teh"), range("recieve")], 0, () => {});
    view.view.dispatch({ selection: EditorSelection.cursor(range("recieve").to) });
    expect(view.markAt()).toBe(1);
    view.view.dispatch({ selection: EditorSelection.cursor(0) });
    expect(view.markAt()).toBeNull();
  });

  it("reveal selects the exact range", () => {
    const view = source(TEXT);
    view.reveal(range("recieve"));
    expect(view.selection()).toBe("recieve");
  });
});
