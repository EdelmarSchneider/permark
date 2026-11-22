// @vitest-environment jsdom
// The reading place of Source: the line at the top of what is in view, also when the page around the editor is what
// scrolls (the editor is as tall as its text inside the page's scroller); its headings for the outline.

import "../visual/jsdomShims";
import { ensureSyntaxTree, forceParsing } from "@codemirror/language";
import { EditorSelection, EditorState, StateEffect } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MOCK_DOCUMENT_PATHS, MOCK_EDIT_SOURCES, mockView } from "../../../api/mockDocs";
import { createSource, headingStarts, languageFor, type SourceHost, type SourceView } from "./cm";
import type { Fence } from "./fence";

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

/** The Markdown node names Source reads over each piece of `text` that is `name`d (`[name, text]` pairs). */
function inlineNodes(text: string): [string, string][] {
  const language = languageFor("notes.md", true);
  if (language instanceof Promise) throw new Error("Markdown loads at once");
  const state = EditorState.create({ doc: text, extensions: [language] });
  const tree = ensureSyntaxTree(state, state.doc.length, 5_000);
  if (!tree) throw new Error("not parsed");
  const found: [string, string][] = [];
  tree.iterate({
    enter: (node) => {
      if (/^(Strikethrough|Highlight|Subscript|Superscript|Emoji)$/.test(node.name)) {
        found.push([node.name, state.doc.sliceString(node.from, node.to)]);
      }
    },
  });
  return found;
}

describe("Source reads inline syntax as View draws it", () => {
  it("single and double tildes strike through, == marks, :name: is an emoji; no sub- or superscript", () => {
    expect(inlineNodes("One ~gone~ and ~~both~~ go, ==bright== stays, :smile: smiles.\n")).toEqual([
      ["Strikethrough", "~gone~"],
      ["Strikethrough", "~~both~~"],
      ["Highlight", "==bright=="],
      ["Emoji", ":smile:"],
    ]);
    // inside a word too, as View strikes it; never in code, around spaces or in runs of three
    expect(inlineNodes("No ^sup^, H~2~O, `==code==`, a == b == c, ===x===\n")).toEqual([
      ["Strikethrough", "~2~"],
    ]);
  });
});

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
    // longer than what the editor parses when it starts (the rest is parsed in the background, and a slow machine
    // stops the first parse earlier): the measure needs the whole tree, as it has once the parse caught up
    const lines = Array.from({ length: 300 }, (_, i) =>
      i % 50 === 0 ? `# Part ${i}` : `line ${i} of the notes`,
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
    expect(forceParsing(cm, cm.state.doc.length, 10_000)).toBe(true);
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
    // read before the jump, as the jump does: the editor's own scroll after it measures again, and the estimated
    // heights of lines never drawn can move by a rounding step
    const brought = at(starts[3] ?? -1) - 50 - 12;
    view.showHeading(3, page);
    expect(page.scrollTop).toBe(brought);
    // no such heading: nothing moves
    view.showHeading(9, page);
    expect(page.scrollTop).toBe(brought);
  });
});

describe("Source insert from outside the editor", () => {
  function make(text: string, readOnly = false): SourceView {
    made?.destroy();
    parent?.remove();
    parent = document.createElement("div");
    document.body.append(parent);
    made = createSource({ parent, text, path: "notes.md", markdown: true, host });
    if (readOnly) {
      made.view.dispatch({
        effects: StateEffect.appendConfig.of(EditorState.readOnly.of(true)),
      });
    }
    return made;
  }

  it("inserts at the caret and replaces a selection, the code points as given, caret after", () => {
    const view = make("one two");
    const cm = view.view;
    cm.dispatch({ selection: EditorSelection.cursor(4) });
    expect(view.replaceSelection("Ω")).toBe(true);
    expect(view.text()).toBe("one Ωtwo");
    cm.dispatch({ selection: EditorSelection.range(5, 8) });
    expect(view.replaceSelection("❤️ ")).toBe(true);
    expect(view.text()).toBe("one Ω❤️ ");
    expect(cm.state.selection.main.head).toBe(view.text().length);
  });

  it("replaces every selection range", () => {
    const view = make("a b c");
    // the editor keeps one range unless multiple selections are allowed
    view.view.dispatch({
      effects: StateEffect.appendConfig.of(EditorState.allowMultipleSelections.of(true)),
    });
    view.view.dispatch({
      selection: EditorSelection.create([
        EditorSelection.cursor(1),
        EditorSelection.range(2, 3),
        EditorSelection.cursor(5),
      ]),
    });
    expect(view.replaceSelection("→")).toBe(true);
    expect(view.text()).toBe("a→ → c→");
  });

  it("groups with typing in the history and leaves the keyboard where it is", () => {
    const view = make("one");
    const cm = view.view;
    const focus = vi.spyOn(cm, "focus");
    const typed: boolean[] = [];
    cm.dispatch({
      selection: EditorSelection.cursor(3),
      effects: StateEffect.appendConfig.of(
        EditorView.updateListener.of((update) => {
          for (const tr of update.transactions) {
            if (tr.docChanged) typed.push(tr.isUserEvent("input.type"));
          }
        }),
      ),
    });
    view.replaceSelection("!");
    expect(focus).not.toHaveBeenCalled();
    expect(cm.hasFocus).toBe(false);
    expect(typed).toEqual([true]);
  });

  it("refuses while composing and in a read-only editor; nothing changes", () => {
    const view = make("one");
    const composing = vi.spyOn(view.view, "composing", "get").mockReturnValue(true);
    expect(view.replaceSelection("x")).toBe(false);
    expect(view.text()).toBe("one");
    composing.mockRestore();
    const locked = make("two", true);
    expect(locked.replaceSelection("x")).toBe(false);
    expect(locked.text()).toBe("two");
  });
});

describe("Source diagram preview source", () => {
  it("the caret in the middle of a block of thousands of lines gives the preview the whole block", () => {
    parent = document.createElement("div");
    document.body.append(parent);
    const body = Array.from({ length: 3000 }, (_, i) => `  node${i} -> node${i + 1};`);
    const text = ["# Graph", "", "```dot", "digraph {", ...body, "}", "```", "", "after"].join(
      "\n",
    );
    const fences: (Fence | null)[] = [];
    made = createSource({
      parent,
      text,
      path: "notes.md",
      markdown: true,
      host: { ...host, fence: (fence) => fences.push(fence) },
    });
    made.view.dispatch({ selection: EditorSelection.cursor(made.view.state.doc.line(1500).from) });
    const fence = fences.at(-1);
    expect(fence?.language).toBe("dot");
    expect(fence?.closed).toBe(true);
    expect(fence?.code).toBe(["digraph {", ...body, "}"].join("\n"));
    expect(fence?.from).toBe(3);
  });
});

describe("Source headings for plain text", () => {
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
