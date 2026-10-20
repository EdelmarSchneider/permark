import { describe, expect, it } from "vitest";
import type { TreeFilter } from "../../api";
import { DEFAULT_FILTER } from "./filter";
import {
  MIN_CODE_FILES,
  MIN_CODE_SHARE,
  isAnswer,
  shouldOfferCode,
  withCode,
  type OfferInput,
} from "./codeOffer";

const base: OfferInput = {
  codeHidden: 12,
  totalFiles: 40,
  filter: DEFAULT_FILTER,
  answered: null,
};

describe("when a project is asked about its code", () => {
  it("is asked when many code files are hidden and they are a fair share of the project", () => {
    expect(shouldOfferCode(base)).toBe(true);
    expect(shouldOfferCode({ ...base, filter: { ...DEFAULT_FILTER, show: "documents" } })).toBe(
      true,
    );
  });

  it("is not asked for a few scripts in a docs project", () => {
    // too few
    expect(shouldOfferCode({ ...base, codeHidden: MIN_CODE_FILES - 1, totalFiles: 6 })).toBe(false);
    expect(shouldOfferCode({ ...base, codeHidden: MIN_CODE_FILES, totalFiles: 20 })).toBe(true);
    // enough, but a small share of a big project
    const total = MIN_CODE_FILES * MIN_CODE_SHARE + 1;
    expect(shouldOfferCode({ ...base, codeHidden: MIN_CODE_FILES, totalFiles: total })).toBe(false);
    expect(shouldOfferCode({ ...base, codeHidden: 6, totalFiles: 300 })).toBe(false);
    // none hidden: nothing to offer
    expect(shouldOfferCode({ ...base, codeHidden: 0, totalFiles: 10 })).toBe(false);
  });

  it("is not asked again after either answer", () => {
    expect(shouldOfferCode({ ...base, answered: "shown" })).toBe(false);
    expect(shouldOfferCode({ ...base, answered: "declined" })).toBe(false);
  });

  it("is not asked when the person's own filter is in force", () => {
    const own = (change: Partial<TreeFilter>): OfferInput => ({
      ...base,
      filter: { ...DEFAULT_FILTER, ...change },
    });
    expect(shouldOfferCode(own({ show: "custom" }))).toBe(false);
    expect(shouldOfferCode(own({ show: "everything" }))).toBe(false);
    expect(shouldOfferCode(own({ patterns: "-docs/" }))).toBe(false);
    expect(shouldOfferCode(own({ patterns: "   " }))).toBe(true);
  });

  it("knows an answer when it reads one back", () => {
    expect([isAnswer("shown"), isAnswer("declined")]).toEqual([true, true]);
    for (const other of [undefined, null, "", "yes", 1, true, {}, ["shown"]]) {
      expect(isAnswer(other)).toBe(false);
    }
  });
});

describe("the filter with code shown", () => {
  it("is custom: the preset's categories and code, everything else kept", () => {
    const filter = withCode({ ...DEFAULT_FILTER, showHidden: true, hideEmptyDirs: false });
    expect(filter.show).toBe("custom");
    expect(filter.categories).toEqual([...DEFAULT_FILTER.categories, "code"]);
    expect(filter.showHidden).toBe(true);
    expect(filter.hideEmptyDirs).toBe(false);
    expect(filter.assetsNearDocsOnly).toBe(true);
    expect(withCode({ ...DEFAULT_FILTER, show: "documents" }).categories).toEqual([
      "markdown",
      "text",
      "code",
    ]);
  });

  it("does not add code twice to a custom filter", () => {
    const custom: TreeFilter = {
      ...DEFAULT_FILTER,
      show: "custom",
      categories: ["markdown", "code"],
    };
    expect(withCode(custom).categories).toEqual(["markdown", "code"]);
    expect(withCode({ ...custom, categories: ["markdown"] }).categories).toEqual([
      "markdown",
      "code",
    ]);
  });
});
