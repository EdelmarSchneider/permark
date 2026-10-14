import { describe, expect, it } from "vitest";
import { mockParseBlocks } from "../mock/mockBlocks";
import type { ParsedBlocks } from "../types";
import {
  BOM,
  blockAtLine,
  buildBlockMap,
  changedBlockCount,
  compose,
  countLineBreaks,
  dominantEol,
  joinBlockMap,
  mapLine,
  matchBlocks,
  tidyBlockText,
  untouchedSlots,
  type BlockMap,
} from "./blockModel";

const mapOf = (text: string): BlockMap => buildBlockMap(text, mockParseBlocks(text));

describe("buildBlockMap", () => {
  it("cuts the text into blocks and separators that give the text back", () => {
    const text = "# Title\n\nOne paragraph\nover two lines.\n\n- a\n- b\n";
    const map = mapOf(text);
    expect(map.blocks.map((b) => [b.kind, b.text])).toEqual([
      ["heading", "# Title"],
      ["paragraph", "One paragraph\nover two lines."],
      ["list", "- a\n- b"],
    ]);
    expect(map.seps).toEqual(["", "\n\n", "\n\n", "\n"]);
    expect(joinBlockMap(map)).toBe(text);
  });

  it("measures blocks in lines", () => {
    const map = mapOf("# T\n\nline one\nline two\n\n\n- x\n");
    expect(map.blocks.map((b) => [b.startLine, b.endLine])).toEqual([
      [0, 0],
      [2, 3],
      [6, 6],
    ]);
  });

  it("reads byte ranges, so characters outside ASCII do not move the cuts", () => {
    const text = "# Olá, 世界 😀\n\nÀ vida é «bela» — ok 😀\n\n> ñandú\n";
    const map = mapOf(text);
    expect(map.blocks.map((b) => b.text)).toEqual([
      "# Olá, 世界 😀",
      "À vida é «bela» — ok 😀",
      "> ñandú",
    ]);
    expect(joinBlockMap(map)).toBe(text);
  });

  it("keeps CRLF inside blocks and a BOM and the final newline in the separators", () => {
    const text = `${BOM}# T\r\n\r\nline one\r\nline two\r\n`;
    const map = mapOf(text);
    expect(map.eol).toBe("\r\n");
    expect(map.seps[0]).toBe(BOM);
    expect(map.blocks[1].text).toBe("line one\r\nline two");
    expect(map.seps[2]).toBe("\r\n");
    expect(joinBlockMap(map)).toBe(text);
  });

  it("has no blocks for a blank text, and keeps the text as the one separator", () => {
    for (const text of ["", "\n", `${BOM}\r\n`]) {
      const map = mapOf(text);
      expect(map.blocks).toEqual([]);
      expect(map.seps).toEqual([text]);
      expect(joinBlockMap(map)).toBe(text);
    }
  });

  it("trusts no range that does not fit the text", () => {
    const text = "# A\n\nB\n";
    const bad: ParsedBlocks[] = [
      { eol: "lf", blocks: [{ startByte: 0, endByte: 99, kind: "heading" }] },
      {
        eol: "lf",
        blocks: [
          { startByte: 4, endByte: 6, kind: "paragraph" },
          { startByte: 0, endByte: 3, kind: "heading" },
        ],
      },
      { eol: "lf", blocks: [{ startByte: 2, endByte: 2, kind: "paragraph" }] },
    ];
    for (const parsed of bad) {
      const map = buildBlockMap(text, parsed);
      expect(map.blocks).toEqual([]);
      expect(joinBlockMap(map)).toBe(text);
    }
    // a range that cuts a character in two
    const cut = buildBlockMap("é\n", {
      eol: "lf",
      blocks: [{ startByte: 1, endByte: 2, kind: "paragraph" }],
    });
    expect(cut.blocks).toEqual([]);
    expect(joinBlockMap(cut)).toBe("é\n");
  });
});

describe("line endings", () => {
  it("counts every kind of terminator once", () => {
    expect(countLineBreaks("a\nb\r\nc\rd")).toBe(3);
    expect(countLineBreaks("a\r\n")).toBe(1);
    expect(countLineBreaks("")).toBe(0);
  });

  it("picks the most common line ending", () => {
    expect(dominantEol("a\nb\n")).toBe("\n");
    expect(dominantEol("a\r\nb\r\n")).toBe("\r\n");
    expect(dominantEol("a\r\nb\nc\n")).toBe("\n");
    expect(dominantEol("no breaks")).toBe("\n");
  });

  it("tidies a block from the editor to the file's line ending", () => {
    expect(tidyBlockText("one\ntwo\n\n", "\r\n")).toBe("one\r\ntwo");
    expect(tidyBlockText("one\r\ntwo", "\n")).toBe("one\ntwo");
    expect(tidyBlockText("", "\n")).toBe("");
  });
});

describe("compose", () => {
  const source = `${BOM}# Title\r\n\r\n\r\nFirst paragraph.\r\n\r\n- a\r\n- b\r\n\r\nLast.\r\n`;

  it("gives the original back when nothing was touched", () => {
    const map = mapOf(source);
    expect(compose(map, untouchedSlots(map))).toBe(source);
  });

  it("rewrites only the edited block and keeps every other byte", () => {
    const map = mapOf(source);
    const slots = untouchedSlots(map);
    slots[1] = { origin: 1, text: "First paragraph, edited.\n" };
    expect(compose(map, slots)).toBe(
      source.replace("First paragraph.", "First paragraph, edited."),
    );
  });

  it("keeps the blank lines the author chose around an edited block", () => {
    const map = mapOf(source); // two blank lines after the title
    const slots = untouchedSlots(map);
    slots[0] = { origin: 0, text: "# Title changed" };
    expect(compose(map, slots)).toBe(source.replace("# Title", "# Title changed"));
  });

  it("puts one blank line between blocks that were not neighbours, in the file's line ending", () => {
    const map = mapOf(source);
    const slots = [
      { origin: 0 },
      { origin: null, text: "A new paragraph." },
      { origin: 2 },
      { origin: 3 },
    ];
    expect(compose(map, slots)).toBe(
      `${BOM}# Title\r\n\r\nA new paragraph.\r\n\r\n- a\r\n- b\r\n\r\nLast.\r\n`,
    );
  });

  it("drops a deleted block and the separator that led to it", () => {
    const map = mapOf("a\n\nb\n\nc\n");
    expect(compose(map, [{ origin: 0 }, { origin: 2 }])).toBe("a\n\nc\n");
    expect(compose(map, [{ origin: 1 }, { origin: 2 }])).toBe("b\n\nc\n");
    expect(compose(map, [])).toBe("\n");
  });

  it("reorders blocks with a blank line between", () => {
    const map = mapOf("a\n\n\nb\n");
    expect(compose(map, [{ origin: 1 }, { origin: 0 }])).toBe("b\n\na\n");
  });

  it("composes into an empty document", () => {
    const map = mapOf("");
    expect(compose(map, [])).toBe("");
    expect(compose(mapOf(BOM), [])).toBe(BOM);
    expect(compose(mapOf("\n\n"), [])).toBe("\n\n");
    expect(compose(map, [{ origin: null, text: "# New" }])).toBe("# New\n");
    const bom = mapOf(BOM);
    expect(compose(bom, [{ origin: null, text: "# New" }])).toBe(`${BOM}# New\n`);
  });

  it("refuses a slot that has neither text nor an original", () => {
    const map = mapOf("a\n");
    expect(() => compose(map, [{ origin: null }])).toThrow();
    expect(() => compose(map, [{ origin: 5 }])).toThrow();
  });
});

describe("matchBlocks", () => {
  it("finds the blocks that kept their text, wherever they moved", () => {
    const base = mapOf("# A\n\none\n\ntwo\n\nthree\n");
    const next = mapOf("# A\n\nNEW\n\none\n\nthree\n");
    const match = matchBlocks(base, next);
    expect(match.origins).toEqual([0, null, 1, 3]);
    expect(match.moved).toEqual([0, 2, null, 3]);
    expect(match.added).toBe(1);
    expect(match.removed).toBe(1);
    expect(changedBlockCount(match)).toBe(1);
  });

  it("counts an edit as one changed block, a deletion as one, and blank lines as none", () => {
    const base = mapOf("a\n\nb\n\nc\n");
    expect(changedBlockCount(matchBlocks(base, mapOf("a\n\nB\n\nc\n")))).toBe(1);
    expect(changedBlockCount(matchBlocks(base, mapOf("a\n\nc\n")))).toBe(1);
    expect(changedBlockCount(matchBlocks(base, mapOf("a\n\nb\n\nc\n\nd\n")))).toBe(1);
    expect(changedBlockCount(matchBlocks(base, mapOf("a\n\n\n\nb\n\nc\n\n\n")))).toBe(0);
    expect(changedBlockCount(matchBlocks(base, mapOf("x\n\nb\n\ny\n")))).toBe(2);
  });

  it("matches equal blocks one by one, never by a hash that could collide", () => {
    const base = mapOf("same\n\nsame\n\nother\n");
    const next = mapOf("same\n\nother\n");
    const match = matchBlocks(base, next);
    expect(match.added).toBe(0);
    expect(match.removed).toBe(1);
  });
});

describe("mapLine", () => {
  const base = mapOf("# T\n\nfirst\nparagraph\n\nsecond\n\nthird\n");

  it("keeps the caret in the same block when lines were added above it", () => {
    const next = mapOf("# T\n\nnew one\n\nnew two\n\nfirst\nparagraph\n\nsecond\n\nthird\n");
    const match = matchBlocks(base, next);
    // line 3 is "paragraph", the second line of the first paragraph
    expect(next.blocks[blockAtLine(next, mapLine(base, next, match, 3))].text).toBe(
      "first\nparagraph",
    );
    expect(mapLine(base, next, match, 3)).toBe(7);
  });

  it("stays in the neighbourhood when the block itself changed", () => {
    const next = mapOf("# T\n\nfirst\nparagraph, rewritten\nand longer\n\nsecond\n\nthird\n");
    const match = matchBlocks(base, next);
    const line = mapLine(base, next, match, 3);
    expect(next.blocks[blockAtLine(next, line)].text).toContain("rewritten");
  });

  it("lands on the nearest block above when the block is gone, and never past the end", () => {
    const next = mapOf("# T\n\nfirst\nparagraph\n");
    const match = matchBlocks(base, next);
    expect(mapLine(base, next, match, 7)).toBeLessThanOrEqual(3);
    expect(mapLine(base, mapOf(""), matchBlocks(base, mapOf("")), 5)).toBe(5);
  });
});
