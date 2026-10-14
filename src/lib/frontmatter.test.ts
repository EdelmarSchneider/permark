import { describe, expect, it } from "vitest";
import { frontmatterValue } from "./frontmatter";

describe("frontmatterValue", () => {
  it("keeps ordinary values as text", () => {
    expect(frontmatterValue("status", "Draft")).toEqual({ text: "Draft" });
    expect(frontmatterValue("updated", " 2026-10-13 ")).toEqual({ text: "2026-10-13" });
  });

  it("splits tag-like keys on commas", () => {
    expect(frontmatterValue("tags", "feed, watcher")).toEqual({ chips: ["feed", "watcher"] });
    expect(frontmatterValue("Keywords", "a,b")).toEqual({ chips: ["a", "b"] });
  });

  it("splits bracketed lists for any key and strips quotes", () => {
    expect(frontmatterValue("authors", "[\"Ana\", 'Bo']")).toEqual({ chips: ["Ana", "Bo"] });
  });

  it("falls back to text for an empty list", () => {
    expect(frontmatterValue("tags", "")).toEqual({ text: "" });
    expect(frontmatterValue("x", "[]")).toEqual({ text: "[]" });
  });
});
