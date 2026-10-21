// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import { spotAt, type SpotEnv } from "./dropTarget";

afterEach(() => document.body.replaceChildren());

/** A view with an Explorer, a document page with an editor, a top bar. */
function view(): { row: HTMLElement; page: HTMLElement; editor: HTMLElement; top: HTMLElement } {
  document.body.innerHTML = `
    <header id="top"></header>
    <aside><div data-drop-explorer><div id="row"></div></div></aside>
    <section id="page"><p id="text"></p><div data-drop-editor><div id="line"></div></div></section>`;
  const byId = (id: string): HTMLElement => document.getElementById(id) as HTMLElement;
  return { row: byId("row"), page: byId("text"), editor: byId("line"), top: byId("top") };
}

function env(element: Element | null, patch: Partial<SpotEnv> = {}): SpotEnv {
  return {
    at: () => element,
    treeFolder: () => "docs/sub",
    editorTakesPictures: () => true,
    ...patch,
  };
}

describe("what is under a drag from outside", () => {
  it("the Explorer list: the folder the list says (the project folder too)", () => {
    const v = view();
    expect(spotAt(5, 5, env(v.row))).toEqual({ area: "explorer", folder: "docs/sub" });
    expect(spotAt(5, 5, env(v.row, { treeFolder: () => "" }))).toEqual({
      area: "explorer",
      folder: "",
    });
  });

  it("the Explorer off its list (its header): not the Explorer", () => {
    const v = view();
    expect(spotAt(5, 5, env(v.row, { treeFolder: () => null }))).toEqual({ area: "other" });
  });

  it("an editor being edited: whether it takes pictures", () => {
    const v = view();
    expect(spotAt(5, 5, env(v.editor))).toEqual({ area: "editor", pictures: true });
    expect(spotAt(5, 5, env(v.editor, { editorTakesPictures: () => false }))).toEqual({
      area: "editor",
      pictures: false,
    });
  });

  it("the page in View, the top bar, nothing at all: anywhere else", () => {
    const v = view();
    expect(spotAt(5, 5, env(v.page))).toEqual({ area: "other" });
    expect(spotAt(5, 5, env(v.top))).toEqual({ area: "other" });
    expect(spotAt(5, 5, env(null))).toEqual({ area: "other" });
  });

  it("asks the document for the element under the point by default", () => {
    const v = view();
    const before = document.elementFromPoint;
    document.elementFromPoint = (): Element => v.editor;
    try {
      expect(spotAt(1, 2, { treeFolder: () => null, editorTakesPictures: () => true }).area).toBe(
        "editor",
      );
    } finally {
      document.elementFromPoint = before;
    }
  });
});
