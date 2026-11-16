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

  it("layers are independent: each keeps its own marks, look and caret answer", () => {
    const view = source(TEXT);
    view.marks([range("teh")], 0, () => {});
    view.marks([range("recieve"), range("cat")], null, () => {}, "other", {
      className: "wavy-mark",
    });
    expect(marked(view)).toEqual(["teh*"]);
    const others = Array.from(view.view.contentDOM.querySelectorAll(".wavy-mark"));
    expect(others.map((el) => el.textContent)).toEqual(["cat", "recieve"]);
    expect(others.map((el) => el.getAttribute("data-mark-layer"))).toEqual(["other", "other"]);
    expect(others.map((el) => el.getAttribute("data-mark-index"))).toEqual(["1", "0"]);
    expect(others[0]?.hasAttribute("data-correction-mark")).toBe(false);
    view.view.dispatch({ selection: EditorSelection.cursor(range("recieve").from + 1) });
    expect(view.markAt()).toBeNull();
    expect(view.markAt("other")).toBe(0);
    // clearing one layer leaves the other
    view.marks([], null, () => {}, "other");
    expect(marked(view)).toEqual(["teh*"]);
    expect(view.view.contentDOM.querySelectorAll(".wavy-mark")).toHaveLength(0);
    view.marks([range("cat")], null, () => {}, "other", { className: "wavy-mark" });
    view.marks([], null, () => {});
    expect(marked(view)).toEqual([]);
    expect(view.view.contentDOM.querySelectorAll(".wavy-mark")).toHaveLength(1);
  });

  it("each layer's marks follow edits", () => {
    const view = source(TEXT);
    view.marks([range("teh")], 0, () => {});
    view.marks([range("recieve")], null, () => {}, "other", { className: "wavy-mark" });
    view.view.dispatch({ changes: { from: 0, insert: "Well, " } });
    const other = view.view.contentDOM.querySelector(".wavy-mark");
    expect(other && view.view.posAtDOM(other)).toBe(range("recieve").from + "Well, ".length);
    const own = view.view.contentDOM.querySelector(".correction-mark");
    expect(own && view.view.posAtDOM(own)).toBe(range("teh").from + "Well, ".length);
  });

  it("a click picks in the clicked mark's layer; a right-click opens that layer's menu", () => {
    const view = source(TEXT);
    const picked: string[] = [];
    const menus: unknown[] = [];
    view.marks([range("teh")], 0, (i) => picked.push(`correction ${i}`));
    view.marks([range("recieve")], null, (i) => picked.push(`other ${i}`), "other", {
      className: "wavy-mark",
      menu: (index, x, y) => menus.push([index, x, y]),
    });
    const other = view.view.contentDOM.querySelector(".wavy-mark");
    other?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
    expect(picked).toEqual(["other 0"]);
    const right = new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: 12,
      clientY: 34,
    });
    other?.dispatchEvent(right);
    expect(menus).toEqual([[0, 12, 34]]);
    expect(right.defaultPrevented).toBe(true);
    // a layer without a menu leaves the webview's own
    const own = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
    view.view.contentDOM.querySelector(".correction-mark")?.dispatchEvent(own);
    expect(own.defaultPrevented).toBe(false);
    expect(menus).toHaveLength(1);
  });

  it("reveal selects the exact range", () => {
    const view = source(TEXT);
    view.reveal(range("recieve"));
    expect(view.selection()).toBe("recieve");
  });
});
