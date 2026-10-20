import { describe, expect, it } from "vitest";
import type { VocabEntry, VocabFailure } from "../../api";
import {
  displayOf,
  failureKey,
  formatMegabytes,
  frontMatterEnd,
  lineCount,
  lineStart,
  percentOf,
  sectionRanges,
} from "./model";
import { infoOf, type TokenizerInfo } from "./tokenizers";

const gpt = infoOf("o200k") as TokenizerInfo;
const claude = infoOf("claude") as TokenizerInfo;
const figure = { value: 12, approx: false };

const entry = (state: VocabEntry["state"]): VocabEntry => ({ state, bytes: 3_613_922 });

describe("front matter", () => {
  it("ends at the line that closes the block", () => {
    expect(frontMatterEnd("---\ntitle: x\n---\n# Doc\n")).toBe(3);
    expect(frontMatterEnd("+++\ntitle = 'x'\n+++\nbody")).toBe(3);
    expect(frontMatterEnd("---\r\na: 1\r\n...\r\nbody")).toBe(3);
    expect(frontMatterEnd("---\nnever closed\n")).toBe(0);
    expect(frontMatterEnd("# no front matter\n---\n")).toBe(0);
    expect(frontMatterEnd("")).toBe(0);
  });

  it("lines start where the line breaks say", () => {
    const text = "a\nbb\nccc";
    expect(lineStart(text, 1)).toBe(0);
    expect(lineStart(text, 2)).toBe(2);
    expect(lineStart(text, 3)).toBe(5);
    expect(lineStart(text, 9)).toBe(text.length);
    expect(lineCount(text)).toBe(3);
    expect(lineCount("")).toBe(1);
    expect(lineCount("a\n")).toBe(2);
  });
});

describe("sections of the outline", () => {
  it("run from the heading to the line before the next of the same or a higher level", () => {
    const items = [
      { level: 1, line: 1 },
      { level: 2, line: 5 },
      { level: 3, line: 8 },
      { level: 2, line: 12 },
    ];
    expect(sectionRanges(items, 20)).toEqual([
      [1, 20],
      [5, 11],
      [8, 11],
      [12, 20],
    ]);
    expect(sectionRanges([], 3)).toEqual([]);
    // two headings on one line (a degenerate page): never an empty or backwards range
    expect(
      sectionRanges(
        [
          { level: 2, line: 4 },
          { level: 2, line: 4 },
        ],
        9,
      ),
    ).toEqual([
      [4, 4],
      [4, 9],
    ]);
  });
});

describe("what the status item shows", () => {
  const base = {
    countable: true,
    primary: gpt,
    vocab: entry({ kind: "ready" }),
    whole: figure,
    selection: null,
  };

  it("nothing without a document, a chip to choose when nothing is chosen", () => {
    expect(displayOf({ ...base, countable: false }).kind).toBe("none");
    expect(displayOf({ ...base, primary: null, vocab: null }).kind).toBe("choose");
  });

  it("an exact tokenizer without its vocabulary shows the action, never a number", () => {
    expect(displayOf({ ...base, vocab: entry({ kind: "absent" }) })).toMatchObject({
      kind: "download",
      bytes: 3_613_922,
    });
    expect(
      displayOf({
        ...base,
        vocab: entry({ kind: "downloading", done: 1_000_000, total: 4_000_000 }),
      }),
    ).toMatchObject({ kind: "downloading", percent: 25 });
    expect(
      displayOf({ ...base, vocab: entry({ kind: "failed", reason: { kind: "offline" } }) }).kind,
    ).toBe("failed");
  });

  it("a ready vocabulary, an estimate and an unanswered backend all show the count", () => {
    expect(displayOf(base)).toMatchObject({ kind: "count", whole: figure });
    expect(displayOf({ ...base, primary: claude, vocab: null }).kind).toBe("count");
    expect(displayOf({ ...base, vocab: null }).kind).toBe("count");
  });

  it("carries the selection beside the whole", () => {
    const part = { value: 3, approx: false };
    expect(displayOf({ ...base, selection: part })).toMatchObject({ selection: part });
  });
});

describe("words for what went wrong", () => {
  it("has a sentence for every failure", () => {
    const failures: VocabFailure[] = [
      { kind: "offline" },
      { kind: "http", status: 404 },
      { kind: "hash" },
      { kind: "disk" },
    ];
    const keys = failures.map(failureKey);
    expect(new Set(keys).size).toBe(4);
  });

  it("percent stays between 0 and 100", () => {
    expect(percentOf(0, 0)).toBe(0);
    expect(percentOf(5, 10)).toBe(50);
    expect(percentOf(99, 10)).toBe(100);
  });

  it("writes a download size in megabytes", () => {
    expect(formatMegabytes(3_613_922, "en")).toBe("3.6 MB");
    expect(formatMegabytes(1_681_126, "de")).toBe("1,7 MB");
  });
});
