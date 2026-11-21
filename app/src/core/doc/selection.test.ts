// @vitest-environment jsdom
import "../testing/media";
import { afterEach, describe, expect, it, vi } from "vitest";
import { documents } from "./document.svelte";
import { pageSelection } from "./selection";

function block(id: string, text: string): HTMLElement {
  const el = document.createElement("p");
  el.setAttribute("data-b", id);
  el.textContent = text;
  document.body.append(el);
  return el;
}

function select(from: Node, to: Node = from): void {
  const range = document.createRange();
  range.setStart(from, 0);
  range.setEnd(to, to.childNodes.length);
  const selection = document.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
}

afterEach(() => {
  vi.restoreAllMocks();
  document.getSelection()?.removeAllRanges();
  document.body.replaceChildren();
});

describe("the page selection", () => {
  it("is its text and the first lines of the blocks it starts and ends in, in page order", () => {
    const first = block("b1", "First");
    const second = block("b2", "Second");
    vi.spyOn(documents, "referenceAt").mockImplementation((node) => ({
      path: "a.md",
      section: null,
      line: node === first || node?.parentElement === first ? 3 : 9,
    }));
    select(first, second);
    expect(pageSelection()).toEqual({ text: "FirstSecond", lines: [3, 9] });
  });

  it("has no lines while the document's blocks are not known", () => {
    const only = block("b1", "Text");
    vi.spyOn(documents, "referenceAt").mockReturnValue(null);
    select(only);
    expect(pageSelection()).toEqual({ text: "Text", lines: null });
  });

  it("is none when collapsed or outside the page", () => {
    const outside = document.createElement("div");
    outside.textContent = "Panel text";
    document.body.append(outside);
    select(outside);
    expect(pageSelection()).toBeNull();
    document.getSelection()?.removeAllRanges();
    expect(pageSelection()).toBeNull();
  });

  it("keeps a triple-clicked last block whose selection ends at the start of what follows the page", () => {
    const first = block("b1", "First");
    const last = block("b2", "Last");
    const panel = document.createElement("nav");
    panel.textContent = "Outline item";
    document.body.append(panel);
    vi.spyOn(documents, "referenceAt").mockImplementation((node) => ({
      path: "a.md",
      section: null,
      line: node === first || node?.parentElement === first ? 3 : 9,
    }));
    const range = document.createRange();
    range.setStart(last.firstChild as Node, 0);
    range.setEnd(panel, 0);
    document.getSelection()?.removeAllRanges();
    document.getSelection()?.addRange(range);
    expect(pageSelection()).toEqual({ text: "Last", lines: [9, 9] });
    // a selection reaching into the panel's own text stays outside the page
    range.setEnd(panel.firstChild as Node, 4);
    document.getSelection()?.removeAllRanges();
    document.getSelection()?.addRange(range);
    expect(pageSelection()).toBeNull();
  });
});
