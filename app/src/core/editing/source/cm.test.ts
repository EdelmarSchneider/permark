// @vitest-environment jsdom
// The reading place of Source: the line at the top of what is in view, also when the page around the editor is what
// scrolls (the editor is as tall as its text inside the page's scroller); its headings for the outline.

import "../visual/jsdomShims";
import { ensureSyntaxTree } from "@codemirror/language";
import { EditorState } from "@codemirror/state";
import { afterEach, describe, expect, it } from "vitest";
import { MOCK_DOCUMENT_PATHS, MOCK_EDIT_SOURCES, mockView } from "../../../api/mockDocs";
import { createSource, headingStarts, languageFor, type SourceHost, type SourceView } from "./cm";

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

/** Lines (1-based) of the headings Source finds in Markdown `text`. */
function headingLines(text: string): number[] {
  const language = languageFor("notes.md", true);
  if (language instanceof Promise) throw new Error("Markdown loads at once");
  const state = EditorState.create({ doc: text, extensions: [language] });
  const tree = ensureSyntaxTree(state, state.doc.length, 5_000);
  if (!tree) throw new Error("not parsed");
  return headingStarts(tree).map((pos) => state.doc.lineAt(pos).number);
}

describe("Source headings for the outline", () => {
  it("finds headings in quotes and lists and setext ones; none in code, front matter or HTML", () => {
    const text = [
      "---",
      "# a comment of the front matter",
      "---",
      "# One",
      "text",
      "> ## Quoted",
      "- ### Listed",
      "```md",
      "# fenced",
      "```",
      "    # indented code",
      "<div>",
      "# in HTML",
      "</div>",
      "",
      "Two",
      "===",
    ].join("\n");
    expect(headingLines(text)).toEqual([4, 6, 7, 16]);
  });

  it("the same headings as the outline of View, for every fixture document", () => {
    let checked = 0;
    for (const path of MOCK_DOCUMENT_PATHS) {
      const view = mockView(path, [], true);
      const source = MOCK_EDIT_SOURCES.get(path);
      if (view?.body.kind !== "markdown" || !source) continue;
      expect(headingLines(source.text), path).toEqual(view.body.outline.map((item) => item.line));
      checked += 1;
    }
    expect(checked).toBeGreaterThan(3);
  });

  it("heading tops are measured from the page's top edge; a heading is brought just below it", () => {
    parent = document.createElement("div");
    document.body.append(parent);
    const lines = Array.from({ length: 300 }, (_, i) =>
      i % 50 === 0 ? `# Part ${i}` : `line ${i}`,
    );
    const view = createSource({
      parent,
      text: lines.join("\n"),
      path: "notes.md",
      markdown: true,
      host,
    });
    made = view;
    const cm = view.view;
    const starts = [0, 50, 100, 150, 200, 250].map((n) => cm.state.doc.line(n + 1).from);
    // the page scrolled 1400 px: the whole editor moved up with it
    cm.scrollDOM.getBoundingClientRect = () => rectAt(-1400);
    cm.contentDOM.getBoundingClientRect = () => rectAt(-1400);
    const at = (pos: number): number => cm.documentTop + cm.lineBlockAt(pos).top;
    expect(view.headingTops(0)).toEqual(starts.map((pos) => at(pos)));
    expect(view.headingTops(100)).toEqual(starts.map((pos) => at(pos) - 100));
    // the editor below the page's edge: what is in view starts at the editor's own top
    cm.scrollDOM.getBoundingClientRect = () => rectAt(200);
    cm.contentDOM.getBoundingClientRect = () => rectAt(200);
    expect(view.headingTops(0)).toEqual(starts.map((pos) => at(pos) - 200));

    const page = document.createElement("div");
    page.getBoundingClientRect = () => rectAt(50);
    page.scrollTop = 0;
    view.showHeading(3, page);
    expect(page.scrollTop).toBe(at(starts[3] ?? -1) - 50 - 12);
    // no such heading: nothing moves
    view.showHeading(9, page);
    expect(page.scrollTop).toBe(at(starts[3] ?? -1) - 50 - 12);
  });

  it("other text has no headings", () => {
    parent = document.createElement("div");
    document.body.append(parent);
    made = createSource({
      parent,
      text: "# not a heading\n",
      path: "run.sh",
      markdown: false,
      host,
    });
    expect(made.headingTops(0)).toEqual([]);
  });
});
