import { describe, expect, it } from "vitest";
import { absorbTokens, hasAllTags, parseTagQuery, unknownTags, withoutPartial } from "./tagQuery";

describe("parseTagQuery", () => {
  it("separates finished tag tokens from the words", () => {
    expect(parseTagQuery("tag:spec live feed ")).toEqual({
      tags: ["spec"],
      text: "live feed",
      partial: null,
    });
    expect(parseTagQuery("live tag:Spec feed tag:needs-review x")).toMatchObject({
      tags: ["spec", "needs-review"],
      text: "live feed x",
    });
  });

  it("treats the token at the end, still being typed, as partial", () => {
    expect(parseTagQuery("live tag:wat")).toEqual({ tags: [], text: "live", partial: "wat" });
    expect(parseTagQuery("tag:")).toEqual({ tags: [], text: "", partial: "" });
    expect(parseTagQuery("tag:wat ")).toEqual({ tags: ["wat"], text: "", partial: null });
  });

  it("repeated tokens are one filter, and empty ones are dropped", () => {
    expect(parseTagQuery("tag:a tag:A tag: x").tags).toEqual(["a"]);
  });

  it("is case-insensitive about the prefix and leaves other words alone", () => {
    expect(parseTagQuery("TAG:spec tags:no mytag:no ")).toEqual({
      tags: ["spec"],
      text: "tags:no mytag:no",
      partial: null,
    });
  });

  it("handles an empty query", () => {
    expect(parseTagQuery("")).toEqual({ tags: [], text: "", partial: null });
    expect(parseTagQuery("   ")).toEqual({ tags: [], text: "", partial: null });
  });
});

describe("absorbTokens", () => {
  it("moves finished tokens into the chips and keeps the token being typed", () => {
    expect(absorbTokens([], "tag:spec foo tag:wa")).toEqual({
      chips: ["spec"],
      text: "foo tag:wa",
      changed: true,
    });
    expect(absorbTokens(["spec"], "tag:needs-review ")).toEqual({
      chips: ["spec", "needs-review"],
      text: "",
      changed: true,
    });
  });

  it("does not repeat a chip", () => {
    expect(absorbTokens(["spec"], "tag:spec x ").chips).toEqual(["spec"]);
  });

  it("leaves the text alone when there is no finished token", () => {
    expect(absorbTokens(["a"], "live feed ")).toEqual({
      chips: ["a"],
      text: "live feed ",
      changed: false,
    });
    expect(absorbTokens([], "tag:sp").changed).toBe(false);
  });
});

describe("withoutPartial", () => {
  it("drops only the token at the end", () => {
    expect(withoutPartial("foo tag:wa")).toBe("foo ");
    expect(withoutPartial("tag:wa")).toBe("");
    expect(withoutPartial("foo tag:a bar")).toBe("foo tag:a bar");
  });
});

describe("unknownTags and hasAllTags", () => {
  it("names the tags no file has", () => {
    expect(unknownTags(["a", "b", "c"], new Set(["a", "c"]))).toEqual(["b"]);
  });

  it("needs every wanted tag, and passes everything when none are wanted", () => {
    expect(hasAllTags(["a", "b"], ["a"])).toBe(true);
    expect(hasAllTags(["a", "b"], ["a", "c"])).toBe(false);
    expect(hasAllTags(undefined, ["a"])).toBe(false);
    expect(hasAllTags(undefined, [])).toBe(true);
  });
});
