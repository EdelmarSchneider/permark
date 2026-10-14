import { describe, expect, it } from "vitest";
import {
  charCount,
  MAX_DESCRIPTION_CHARS,
  MAX_TAG_CHARS,
  normalizeDescription,
  normalizeTag,
  normalizeTags,
  splitTagInput,
} from "./tagText";

describe("normalizeTag", () => {
  it("trims, lowercases and joins inner whitespace with a hyphen", () => {
    expect(normalizeTag("  Needs Review ")).toBe("needs-review");
    expect(normalizeTag("a \t b")).toBe("a-b");
    expect(normalizeTag("SPEC")).toBe("spec");
  });

  it("keeps slashes as text", () => {
    expect(normalizeTag("Area/API")).toBe("area/api");
  });

  it("returns null when nothing is left", () => {
    expect(normalizeTag("")).toBeNull();
    expect(normalizeTag("  \n ")).toBeNull();
  });

  it("cuts to 40 characters, counting characters and not code units", () => {
    expect(normalizeTag("x".repeat(60))).toBe("x".repeat(MAX_TAG_CHARS));
    const emoji = normalizeTag("😀".repeat(60)) ?? "";
    expect(charCount(emoji)).toBe(MAX_TAG_CHARS);
  });
});

describe("normalizeTags", () => {
  it("drops empty tags and repeats, keeping the first place", () => {
    expect(normalizeTags(["Spec", " ", "feed", "SPEC", "New Idea"])).toEqual([
      "spec",
      "feed",
      "new-idea",
    ]);
  });
});

describe("normalizeDescription", () => {
  it("makes one line and trims", () => {
    expect(normalizeDescription("  A\nB\r\n\r\nC\tD ")).toBe("A B C D");
    expect(normalizeDescription(" \n ")).toBe("");
  });

  it("cuts to 120 characters", () => {
    expect(charCount(normalizeDescription("x".repeat(300)))).toBe(MAX_DESCRIPTION_CHARS);
  });
});

describe("splitTagInput", () => {
  it("finishes the tags before a comma and keeps the rest as typed", () => {
    expect(splitTagInput("spec, Needs Review, wat")).toEqual({
      done: ["spec", "needs-review"],
      rest: " wat",
    });
    expect(splitTagInput("spec,")).toEqual({ done: ["spec"], rest: "" });
    expect(splitTagInput("spec")).toEqual({ done: [], rest: "spec" });
    expect(splitTagInput(",, ,")).toEqual({ done: [], rest: "" });
  });
});
