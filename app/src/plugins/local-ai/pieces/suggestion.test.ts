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

describe("the excerpt as the Edit view shows the text", () => {
  /** The shown excerpt around the `nth` (from 0) `word` of `doc`, as one line: `before[word]after`. */
  function shown(doc: string, word: string, nth = 0, kind?: "cell"): string {
    let at = -1;
    for (let i = 0; i <= nth; i++) at = doc.indexOf(word, at + 1);
    const { before, after } = around(doc, at, at + word.length, kind, "shown");
    return `${before}[${word}]${after}`;
  }

  it("leaves out the marks the Edit view draws instead of showing: quote, emphasis, code, links, headings, lists", () => {
    const quote = "> This is **very** important and teh text was _quite_ wrong.\n";
    expect(shown(quote, "teh")).toBe("This is very important and [teh] text was quite wrong.\n");
    // written: as ever
    const at = quote.indexOf("teh");
    expect(around(quote, at, at + 3).before).toBe("> This is **very** important and ");
    expect(shown("Some ***teh*** word ~~gone~~ here", "teh")).toBe("Some [teh] word gone here");
    expect(shown("Run `npm i` and teh rest", "teh")).toBe("Run npm i and [teh] rest");
    expect(shown("See [the docs](/a 'Title') and teh end", "teh")).toBe(
      "See the docs and [teh] end",
    );
    expect(shown("Mail <me@x.y> or <b>bold</b> teh", "teh")).toBe("Mail me@x.y or bold [teh]");
    expect(shown("## A teh title ##", "teh")).toBe("A [teh] title ");
    expect(shown("- first item\n- [ ] second teh\n  more", "teh")).toBe(
      "first item\nsecond [teh]\n  more",
    );
    expect(shown("> one line\n> two teh\n", "teh")).toBe("one line\ntwo [teh]\n");
    // an escaped mark is shown as the mark
    expect(shown("Not \\*bold\\* teh", "teh")).toBe("Not *bold* [teh]");
  });

  it("cut by size as the text is written (word edges, `…`, never past a blank line)", () => {
    const long = `${"word ".repeat(12)}**strong** teh ${"more ".repeat(12)}`;
    expect(shown(long, "teh")).toBe(
      `…${"word ".repeat(5)}strong [teh] ${"more ".repeat(8).trim()}…`,
    );
    expect(shown("**Kept.**\n\nNew **teh** block\n\n_next_", "teh")).toBe("New [teh] block");
    // a bold run whose start the size cuts off: its end is no mark of its own
    const cut = `${"x ".repeat(30)}**${"bold ".repeat(10)}teh bold** end`;
    const at = cut.indexOf("teh");
    expect(around(cut, at, at + 3).after).toBe(" bold** end");
    expect(shown(cut, "teh")).toBe(`…${"bold ".repeat(8)}[teh] bold end`);
  });

  it("in a table cell: the cell's own text, its marks left out", () => {
    const doc = "| Name | Note |\n|---|---|\n| a **b** teh `c` | a \\| d teh |\n";
    expect(shown(doc, "teh", 0, "cell")).toBe("a b [teh] c");
    expect(shown(doc, "teh", 1, "cell")).toBe("a | d [teh]");
  });
});
