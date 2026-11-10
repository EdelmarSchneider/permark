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
});
