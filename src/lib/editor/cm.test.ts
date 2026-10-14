// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import "./visual/jsdomShims";
import { createSourceView, type SourceView } from "./cm";

const handlers = { onChange: () => {}, onCaret: () => {}, onFiles: () => {} };
let view: SourceView | null = null;

function open(text: string, onChange: (length: number) => void = () => {}): SourceView {
  const parent = document.createElement("div");
  document.body.appendChild(parent);
  view = createSourceView(parent, text, 0, "test", { ...handlers, onChange });
  return view;
}

afterEach(() => {
  view?.destroy();
  view = null;
  document.body.innerHTML = "";
});

describe("the Source editor and the Outline", () => {
  const TEXT =
    "---\ntags: [a]\n---\n# Title\n\nIntro.\n\n## Sub\n\n```\n# not a heading\n```\n\nEnd\n\n---\n";

  it("lists the headings of its text", () => {
    expect(open(TEXT).headings()).toEqual([
      { level: 1, text: "Title" },
      { level: 2, text: "Sub" },
    ]);
  });

  it("lists the headings of the text as it is after an edit", () => {
    const editor = open("# One\n\ntext\n");
    expect(editor.headings()).toHaveLength(1);
    editor.setText("# One\n\n## Two\n\n### Three\n", 0);
    expect(editor.headings().map((h) => h.text)).toEqual(["One", "Two", "Three"]);
  });

  it("ignores a heading index that does not exist", () => {
    const editor = open("# One\n");
    expect(() => editor.scrollToHeading(5)).not.toThrow();
    expect(() => editor.scrollToHeading(0)).not.toThrow();
  });

  it("has no current section without headings", () => {
    expect(open("just text\n").currentHeading()).toBeNull();
  });

  it("tells the Outline when the person scrolls or types, until stopped", () => {
    const editor = open("# One\n");
    const seen = vi.fn();
    const stop = editor.onViewport(seen);
    const scroller = document.querySelector(".cm-scroller") as HTMLElement;
    scroller.dispatchEvent(new Event("scroll"));
    expect(seen).toHaveBeenLastCalledWith(false);
    scroller.dispatchEvent(new Event("wheel"));
    expect(seen).toHaveBeenLastCalledWith(true);
    (document.querySelector(".cm-content") as HTMLElement).dispatchEvent(
      new Event("keydown", { bubbles: true }),
    );
    expect(seen).toHaveBeenCalledTimes(3);
    stop();
    scroller.dispatchEvent(new Event("scroll"));
    expect(seen).toHaveBeenCalledTimes(3);
  });

  it("reports edits to the store (the Outline rebuilds from them)", () => {
    const onChange = vi.fn();
    const editor = open("# One\n", onChange);
    editor.setText("# Two\n", 0);
    expect(onChange).toHaveBeenCalled();
  });
});

describe("the Source editor and the place it keeps", () => {
  const TEXT = "# One\n\nfirst\n\nsecond\n\nthird\n";

  it("opens with the caret on the line asked for, and a top line does not move it", () => {
    const parent = document.createElement("div");
    document.body.appendChild(parent);
    view = createSourceView(parent, TEXT, 4, "test", handlers, false, 2);
    expect(view.caretLine()).toBe(4);
    expect(view.getText()).toBe(TEXT);
  });

  it("reports the line at the top of the view (the first one without a layout)", () => {
    expect(open(TEXT).topLine()).toBe(0);
  });
});
