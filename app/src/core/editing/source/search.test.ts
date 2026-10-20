// @vitest-environment jsdom
// Find + replace and go to line in Source (CodeMirror's search panel), in the app's language.

import {
  closeSearchPanel,
  findNext,
  gotoLine,
  openSearchPanel,
  replaceAll,
  searchPanelOpen,
  setSearchQuery,
  SearchQuery,
} from "@codemirror/search";
import { afterEach, describe, expect, it } from "vitest";
import { createSource, type SourceHost, type SourceView } from "./cm";
import { SEARCH_PHRASES } from "./phrases";
import en from "../../i18n/locales/en";

const host: SourceHost = {
  changed: () => {},
  selected: () => {},
  save: () => {},
  modeKey: () => {},
  pictures: () => {},
  docPath: () => "notes.txt",
  linkTargets: async () => [],
  headings: async () => [],
};

let made: SourceView | null = null;
let parent: HTMLElement | null = null;

function source(text: string, phrases?: Record<string, string>): SourceView {
  parent = document.createElement("div");
  document.body.append(parent);
  made = createSource({ parent, text, path: "notes.txt", markdown: false, host, phrases });
  return made;
}

afterEach(() => {
  made?.destroy();
  made = null;
  parent?.remove();
  parent = null;
});

describe("Source find panel", () => {
  it("opens with the app's words, finds, replaces, closes", () => {
    const words = Object.fromEntries(SEARCH_PHRASES.map(([p, key]) => [p, `«${en[key]}»`]));
    const s = source("alpha beta\nAlpha gamma\nalpha\n", words);
    expect(openSearchPanel(s.view)).toBe(true);
    expect(searchPanelOpen(s.view.state)).toBe(true);
    const panel = parent?.querySelector(".cm-search");
    expect(panel).not.toBeNull();
    expect(panel?.querySelector("input[name=search]")?.getAttribute("placeholder")).toBe("«Find»");
    expect(panel?.textContent).toContain("«Match case»");
    expect(panel?.textContent).toContain("«Regular expression»");
    // case-insensitive by default: three matches; case: two
    s.view.dispatch({ effects: setSearchQuery.of(new SearchQuery({ search: "alpha" })) });
    expect(findNext(s.view)).toBe(true);
    expect(s.selection()).toBe("alpha");
    s.view.dispatch({
      effects: setSearchQuery.of(
        new SearchQuery({ search: "alpha", caseSensitive: true, replace: "omega" }),
      ),
    });
    replaceAll(s.view);
    expect(s.text()).toBe("omega beta\nAlpha gamma\nomega\n");
    // regular expression with a group
    s.view.dispatch({
      effects: setSearchQuery.of(
        new SearchQuery({ search: "(\\w+) gamma", regexp: true, replace: "$1 delta" }),
      ),
    });
    replaceAll(s.view);
    expect(s.text()).toBe("omega beta\nAlpha delta\nomega\n");
    expect(closeSearchPanel(s.view)).toBe(true);
    expect(parent?.querySelector(".cm-search")).toBeNull();
  });

  it("go to line opens its own field in the app's words", () => {
    const words = Object.fromEntries(SEARCH_PHRASES.map(([p, key]) => [p, `«${en[key]}»`]));
    const s = source("one\ntwo\nthree\n", words);
    expect(gotoLine(s.view)).toBe(true);
    const dialog = parent?.querySelector(".cm-gotoLine, .cm-goto-line");
    expect(dialog?.textContent).toContain("«Go to line»");
  });

  it("every phrase has a translation key in English", () => {
    for (const [, key] of SEARCH_PHRASES) expect(en[key]).toBeTruthy();
  });

  it("shows invisible characters instead of hiding them", () => {
    const s = source("a\u0000b​c\n");
    expect(s.text()).toBe("a\u0000b​c\n");
    expect(parent?.querySelectorAll(".cm-specialChar").length).toBeGreaterThanOrEqual(2);
  });
});
