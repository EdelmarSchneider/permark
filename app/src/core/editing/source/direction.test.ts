// @vitest-environment jsdom
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { describe, expect, it } from "vitest";
import { lineDirection } from "./direction";

describe("text direction in Source", () => {
  it("lets every line read its own direction, and the caret follow it", () => {
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: "مرحبا بالعالم\nHello world\n- שלום\n",
        extensions: [lineDirection],
      }),
    });
    try {
      expect(view.state.facet(EditorView.perLineTextDirection)).toBe(true);
      const lines = [...view.contentDOM.querySelectorAll(".cm-line")];
      expect(lines.map((line) => line.textContent)).toEqual([
        "مرحبا بالعالم",
        "Hello world",
        "- שלום",
        "",
      ]);
      for (const line of lines) expect(line.getAttribute("dir")).toBe("auto");
      // a typed line gets it too
      view.dispatch({ changes: { from: view.state.doc.length, insert: "עוד" } });
      expect(view.contentDOM.querySelectorAll(".cm-line[dir=auto]")).toHaveLength(4);
    } finally {
      view.destroy();
      parent.remove();
    }
  });
});
