import { describe, expect, it } from "vitest";
import {
  COLORS,
  ICONS,
  MAX_DESCRIPTION,
  MAX_TAG_CHARS,
  filterCounts,
  isColor,
  isIcon,
  normalizeDescription,
  normalizeTag,
  normalizeTags,
  splitTyped,
  suggest,
  tipOf,
} from "./model";

const counts = [
  { name: "spec", count: 5, onlyInFiles: false, status: null, owner: null },
  { name: "needs-review", count: 3, onlyInFiles: false, status: null, owner: null },
  { name: "design", count: 2, onlyInFiles: true, status: null, owner: null },
  { name: "speculation", count: 1, onlyInFiles: false, status: null, owner: null },
];

describe("how a typed tag is cleaned (the backend's rules)", () => {
  it("trims, lowercases and joins words with a hyphen", () => {
    expect(normalizeTag("  Needs Review ")).toBe("needs-review");
    expect(normalizeTag("a \t\n b")).toBe("a-b");
    expect(normalizeTag("area/api")).toBe("area/api");
    expect(normalizeTag("Ünï 档案")).toBe("ünï-档案");
    expect(normalizeTag("   ")).toBeNull();
    expect(normalizeTag("")).toBeNull();
  });

  it("cuts by characters, not by bytes", () => {
    expect([...(normalizeTag("é".repeat(60)) ?? "")]).toHaveLength(MAX_TAG_CHARS);
    expect([...(normalizeTag("😀".repeat(60)) ?? "")]).toHaveLength(MAX_TAG_CHARS);
  });

  it("drops empty tags and repeats, in order, and caps a file's tags", () => {
    expect(normalizeTags(["Spec", "feed", "spec", "FEED", "new idea", " "])).toEqual([
      "spec",
      "feed",
      "new-idea",
    ]);
    expect(normalizeTags(Array.from({ length: 500 }, (_, n) => `t${n}`))).toHaveLength(100);
  });

  it("keeps a description on one line, cut to its limit", () => {
    expect(normalizeDescription("  A\nB\r\n\r\nC\tD  ")).toBe("A B C D");
    expect([...normalizeDescription("日".repeat(500))]).toHaveLength(MAX_DESCRIPTION);
  });
});

describe("the palette", () => {
  it("knows its colors and icons and nothing else", () => {
    expect(COLORS).toHaveLength(8);
    expect(ICONS).toHaveLength(12);
    expect(isColor("blue")).toBe(true);
    expect(isColor("magenta")).toBe(false);
    expect(isColor(null)).toBe(false);
    expect(isIcon("book-open")).toBe(true);
    expect(isIcon("rocket")).toBe(false);
  });
});

describe("the tag field", () => {
  it("a comma ends a tag", () => {
    expect(splitTyped("alpha, Beta Gamma,")).toEqual({ done: ["alpha", "beta-gamma"], rest: "" });
    expect(splitTyped("alpha, be")).toEqual({ done: ["alpha"], rest: " be" });
    expect(splitTyped("alp")).toEqual({ done: [], rest: "alp" });
    expect(splitTyped(", ,")).toEqual({ done: [], rest: "" });
  });

  it("suggests the unused tags: names that start with the words first, then the ones that contain them", () => {
    expect(suggest(counts, "spec", []).map((c) => c.name)).toEqual(["spec", "speculation"]);
    expect(suggest(counts, "spec", ["spec"]).map((c) => c.name)).toEqual(["speculation"]);
    expect(suggest(counts, "view", []).map((c) => c.name)).toEqual(["needs-review"]);
    expect(suggest(counts, "", ["spec"]).map((c) => c.name)).toEqual([
      "needs-review",
      "design",
      "speculation",
    ]);
    expect(suggest(counts, "", [], 2)).toHaveLength(2);
    expect(suggest(counts, "zzz", [])).toEqual([]);
  });
});

describe("the tags view", () => {
  it("filters by words that all occur in the name", () => {
    expect(filterCounts(counts, "").map((c) => c.name)).toHaveLength(4);
    expect(filterCounts(counts, "spec").map((c) => c.name)).toEqual(["spec", "speculation"]);
    expect(filterCounts(counts, "needs rev").map((c) => c.name)).toEqual(["needs-review"]);
    expect(filterCounts(counts, "nothing")).toEqual([]);
  });

  it("writes the tooltip of a row", () => {
    expect(tipOf(["a", "b"], "About it", "Tags")).toBe("Tags: a, b\nAbout it");
    expect(tipOf([], "Only text", "Tags")).toBe("Only text");
    expect(tipOf(["a"], "", "Tags")).toBe("Tags: a");
    expect(tipOf([], "", "Tags")).toBe("");
  });
});
