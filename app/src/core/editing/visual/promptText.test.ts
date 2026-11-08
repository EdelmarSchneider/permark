import { describe, expect, it } from "vitest";
import fixture from "./promptText.fixture.json";
import { promptPieces, type PromptPiece } from "./promptText";

const escape = (text: string): string =>
  text.replace(
    /[&<>"]/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] ?? c,
  );

/** The renderer's URL path: ASCII letters, digits and `/-_.~@+` as they are, every other byte `%XX`. */
function encode(path: string): string {
  let out = "";
  for (const byte of new TextEncoder().encode(path)) {
    const c = String.fromCharCode(byte);
    out += /[A-Za-z0-9/\-_.~@+]/.test(c)
      ? c
      : `%${byte.toString(16).toUpperCase().padStart(2, "0")}`;
  }
  return out;
}

/** The pieces drawn the way the renderer writes them, so both sides compare as one string. */
function render(text: string, pieces: PromptPiece[], files: readonly string[]): string | null {
  let out = "";
  let plain = 0;
  let found = false;
  for (const piece of pieces) {
    if (piece.kind === "file" && !files.includes(piece.path)) continue;
    out += escape(text.slice(plain, piece.from));
    out +=
      piece.kind === "variable"
        ? `<span class="tvar">{{${escape(piece.name)}}}</span>`
        : `<a class="at-ref" href="/${encode(piece.path)}">@${escape(text.slice(piece.from + 1, piece.to))}</a>`;
    plain = piece.to;
    found = true;
  }
  return found ? out + escape(text.slice(plain)) : null;
}

describe("promptPieces", () => {
  it("marks what the renderer marks (shared fixture)", () => {
    expect(fixture.cases.length).toBeGreaterThanOrEqual(30);
    for (const { text, html } of fixture.cases) {
      expect(render(text, promptPieces(text), fixture.files), text).toBe(html);
    }
  });

  it("gives offsets in the text and the project path a reference names", () => {
    expect(promptPieces("a {{ x }} @docs\\plan.md.")).toEqual([
      { kind: "variable", from: 2, to: 9, name: "x" },
      { kind: "file", from: 10, to: 23, path: "docs/plan.md" },
    ]);
  });
});
