// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import { spotAt, type SpotEnv } from "./dropTarget";

afterEach(() => document.body.replaceChildren());

/** A window with an Explorer, a document page with an editor, a top bar. */
function window_(): {
  explorer: HTMLElement;
  page: HTMLElement;
  editor: HTMLElement;
  top: HTMLElement;
} {
  document.body.innerHTML = `
    <header id="top"></header>
    <aside><div data-drop-explorer id="explorer"><div id="row"></div></div></aside>
    <section data-drop-page id="page"><p id="text"></p><div data-drop-editor id="editor"><div id="line"></div></div></section>`;
  const byId = (id: string): HTMLElement => document.getElementById(id) as HTMLElement;
  return { explorer: byId("row"), page: byId("text"), editor: byId("line"), top: byId("top") };
}

function env(element: Element | null, patch: Partial<SpotEnv> = {}): SpotEnv {
  return {
    at: () => element,
    treeFolder: () => "docs/sub",
    document: () => "notes/deep/a.md",
    ...patch,
  };
}

describe("where a drop lands", () => {
  it("in the Explorer: the folder the list says", () => {
    const w = window_();
    expect(spotAt(5, 5, env(w.explorer))).toEqual({ folder: "docs/sub", editor: false });
  });

  it("in the Explorer but off the rows (header, empty space): the project folder", () => {
    const w = window_();
    expect(spotAt(5, 5, env(w.explorer, { treeFolder: () => null }))).toEqual({
      folder: "",
      editor: false,
    });
  });

  it("on the page of the open document: that document's folder", () => {
    const w = window_();
    expect(spotAt(5, 5, env(w.page))).toEqual({ folder: "notes/deep", editor: false });
  });

  it("on the page with no document: the project folder", () => {
    const w = window_();
    expect(spotAt(5, 5, env(w.page, { document: () => null }))).toEqual({
      folder: "",
      editor: false,
    });
  });

  it("on an editor: the document's folder, and it says so", () => {
    const w = window_();
    expect(spotAt(5, 5, env(w.editor))).toEqual({ folder: "notes/deep", editor: true });
  });

  it("a document at the top of the project lands in the project folder", () => {
    const w = window_();
    expect(spotAt(5, 5, env(w.page, { document: () => "README.md" })).folder).toBe("");
  });

  it("a worktree document lands in its worktree", () => {
    const w = window_();
    expect(spotAt(5, 5, env(w.page, { document: () => "@wt/feature/a.md" })).folder).toBe(
      "@wt/feature",
    );
  });

  it("anywhere else (top bar, nothing under the pointer): the project folder", () => {
    const w = window_();
    expect(spotAt(5, 5, env(w.top))).toEqual({ folder: "", editor: false });
    expect(spotAt(5, 5, env(null))).toEqual({ folder: "", editor: false });
  });

  it("asks the document for the element when none is given", () => {
    window_();
    // jsdom has no hit testing: elementFromPoint answers null, so the project folder
    document.elementFromPoint = () => null;
    expect(spotAt(1, 1, { treeFolder: () => "x", document: () => "a/b.md" })).toEqual({
      folder: "",
      editor: false,
    });
  });
});
