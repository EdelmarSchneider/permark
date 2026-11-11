import { describe, expect, it } from "vitest";
import { around } from "./suggestion";

describe("the excerpt around a suggestion", () => {
  it("the excerpt never crosses a blank line nor starts | ends mid-word", () => {
    const doc = "# Title\n\nI has a cat.\n \nNext block";
    const at = doc.indexOf("has");
    expect(around(doc, at, at + 3)).toEqual({ before: "I ", after: " a cat." });
    const long =
      "Wordy sentence leading up to the spot where teh error sits and goes on further away from here today";
    const e = long.indexOf("teh");
    const { before, after } = around(long, e, e + 3);
    expect(before).toBe("…sentence leading up to the spot where ");
    // cut by size: back to a word end, marked
    expect(after).toBe(" error sits and goes on further away…");
    // no space to cut at (CJK): the cut stays, marked
    const ja = "あ".repeat(60) + "X" + "い".repeat(60);
    const x = ja.indexOf("X");
    expect(around(ja, x, x + 1)).toEqual({
      before: `…${"あ".repeat(40)}`,
      after: `${"い".repeat(40)}…`,
    });
  });

  it("in a table cell the excerpt stays inside the cell: no pipe, no padding, nothing of another cell | row", () => {
    const doc = [
      "| Feature  | State        | Notes        |",
      "|----------|--------------|--------------|",
      "| Math     | is donne now | KaTeX, fast  |",
      "| Diagrams | donne        | a \\| b teh c |",
      "",
    ].join("\n");
    const first = doc.indexOf("donne");
    expect(around(doc, first, first + 5, "cell")).toEqual({ before: "is ", after: " now" });
    // read as any other text, it runs over the row
    expect(around(doc, first, first + 5).before).toContain("| Math");
    expect(around(doc, first, first + 5).after).toContain("| KaTeX");
    // the whole cell: nothing around it
    const alone = doc.indexOf("donne", first + 1);
    expect(around(doc, alone, alone + 5, "cell")).toEqual({ before: "", after: "" });
    // an escaped pipe is the cell's own text
    const teh = doc.indexOf("teh");
    expect(around(doc, teh, teh + 3, "cell")).toEqual({ before: "a \\| b ", after: " c" });
    // the other kinds of piece: as without one
    for (const kind of ["paragraph", "heading", "listItem", "quote"] as const)
      expect(around(doc, first, first + 5, kind)).toEqual(around(doc, first, first + 5));
  });

  it("a long cell is cut by size as any text is, inside the cell", () => {
    const doc = `| ${"word ".repeat(12)}teh ${"more ".repeat(12)}| next |\n|---|---|\n`;
    const teh = doc.indexOf("teh");
    expect(around(doc, teh, teh + 3, "cell")).toEqual({
      before: `…${"word ".repeat(8)}`,
      after: ` ${"more ".repeat(8).trim()}…`,
    });
  });

  it("a row written without its outer pipes, in a quote too: the cell ends with the line", () => {
    const doc = "teh a | b\n--|--\none | two teh\n";
    expect(around(doc, 4, 5, "cell")).toEqual({ before: "teh ", after: "" });
    const two = doc.indexOf("two");
    expect(around(doc, two, two + 3, "cell")).toEqual({ before: "", after: " teh" });
    const quoted = "> teh a | b\n> --|--\n";
    expect(around(quoted, 6, 7, "cell")).toEqual({ before: "teh ", after: "" });
  });
});
