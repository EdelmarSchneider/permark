// @vitest-environment jsdom
// The reading place of Source: the line at the top of what is in view, also when the page around the editor is what
// scrolls (the editor is as tall as its text inside the page's scroller).

import "../visual/jsdomShims";
import { afterEach, describe, expect, it } from "vitest";
import { createSource, type SourceHost, type SourceView } from "./cm";

const host: SourceHost = {
  changed: () => {},
  selected: () => {},
  save: () => {},
  modeKey: () => {},
  pictures: () => {},
  fence: () => {},
  docPath: () => "notes.md",
  linkTargets: async () => [],
  headings: async () => [],
};

let made: SourceView | null = null;
let parent: HTMLElement | null = null;

afterEach(() => {
  made?.destroy();
  made = null;
  parent?.remove();
  parent = null;
});

function rectAt(top: number): DOMRect {
  return {
    top,
    bottom: top + 10_000,
    left: 0,
    right: 800,
    width: 800,
    height: 10_000,
    x: 0,
    y: top,
    toJSON: () => ({}),
  };
}

describe("Source reading place", () => {
  it("the page scrolled past the editor's top: the top line is the one under the page's top edge", () => {
    parent = document.createElement("div");
    document.body.append(parent);
    const text = Array.from({ length: 400 }, (_, i) => `line ${i + 1}`).join("\n");
    const view = createSource({ parent, text, path: "notes.md", markdown: true, host });
    made = view;
    // jsdom has no layout: lines are 20 px tall here, the editor's text starts at its top
    const asked: number[] = [];
    view.view.lineBlockAtHeight = (height) => {
      asked.push(height);
      const line = view.view.state.doc.line(
        Math.min(400, Math.max(1, Math.floor(height / 20) + 1)),
      );
      return view.view.lineBlockAt(line.from);
    };
    // the page scrolled 1400 px: the whole editor moved up with it
    view.view.scrollDOM.getBoundingClientRect = () => rectAt(-1400);
    view.view.contentDOM.getBoundingClientRect = () => rectAt(-1400);
    const top = view.view.documentTop;
    expect(view.topLine(0)).toBe(Math.floor((0 - top + 1) / 20) + 1);
    expect(view.topLine(0)).toBeGreaterThan(60);
    // not scrolled: the first line
    view.view.scrollDOM.getBoundingClientRect = () => rectAt(0);
    view.view.contentDOM.getBoundingClientRect = () => rectAt(0);
    expect(view.topLine(0)).toBe(1);
    // the editor scrolls itself (no page around it): its own top edge
    expect(view.topLine()).toBe(1);
  });
});
