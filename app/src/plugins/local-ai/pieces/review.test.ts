// The pure parts of the piece review. The review itself (runs, news, accept | reject, marks) is driven where a plugin
// configures it: `plugins/correction/store.test.ts`.
import { describe, expect, it } from "vitest";
import { changedRange } from "./review.svelte";

describe("changed ranges", () => {
  it("prefix and suffix kept, the rest is the change", () => {
    expect(changedRange("abcdef", "abXdef")).toEqual({ from: 2, to: 3, delta: 0 });
    expect(changedRange("abc", "abcd")).toEqual({ from: 3, to: 3, delta: 1 });
    expect(changedRange("aaaa", "aa")).toEqual({ from: 2, to: 4, delta: -2 });
  });
});
