import { describe, expect, it } from "vitest";
import { SearchTags } from "./tags.svelte";

const provider = {
  list: () => [
    { name: "spec", count: 5 },
    { name: "speculation", count: 1 },
    { name: "needs-review", count: 3 },
  ],
};

describe("the tags a search is narrowed to", () => {
  it("adds, removes and toggles, without repeats", () => {
    const tags = new SearchTags();
    tags.add("a");
    tags.add("a");
    tags.add("b");
    expect(tags.selected).toEqual(["a", "b"]);
    tags.toggle("a");
    expect(tags.selected).toEqual(["b"]);
    tags.toggle("c");
    tags.remove("b");
    expect(tags.selected).toEqual(["c"]);
    tags.clear();
    expect(tags.selected).toEqual([]);
  });

  it("is available while a provider is given, and forgets the selection when it goes", () => {
    const tags = new SearchTags();
    expect(tags.available).toBe(false);
    tags.setProvider(provider);
    tags.add("spec");
    expect(tags.available).toBe(true);
    tags.setProvider(null);
    expect(tags.available).toBe(false);
    expect(tags.selected).toEqual([]);
    expect(tags.hits("spec")).toEqual([]);
  });

  it("matches tag names against the words, best first", () => {
    const tags = new SearchTags();
    tags.setProvider(provider);
    expect(tags.hits("").map((h) => h.name)).toEqual(["spec", "speculation", "needs-review"]);
    expect(tags.hits("spec").map((h) => h.name)).toEqual(["spec", "speculation"]);
    expect(tags.hits("review").map((h) => h.name)).toEqual(["needs-review"]);
    expect(tags.hits("zzz")).toEqual([]);
  });
});
