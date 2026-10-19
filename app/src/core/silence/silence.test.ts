import { describe, expect, it } from "vitest";
import { SilenceStore, isQuiet, wakingMarks, type Marks } from "./silence.svelte";

describe("isQuiet", () => {
  it("speaks until a mark says otherwise, and the closest mark wins", () => {
    const marks: Marks = { src: "quiet", "src/docs": "speaking" };
    expect(isQuiet("a.md", marks)).toBe(false);
    expect(isQuiet("", marks)).toBe(false);
    expect(isQuiet("src", marks)).toBe(true);
    expect(isQuiet("src/deep/er/x.md", marks)).toBe(true);
    expect(isQuiet("src/docs", marks)).toBe(false);
    expect(isQuiet("src/docs/guide.md", marks)).toBe(false);
    expect(isQuiet("srcx/a.md", marks)).toBe(false);
  });

  it("treats a worktree as a root of its own", () => {
    expect(isQuiet("@wt/f/a.md", { "": "quiet" })).toBe(false);
    expect(isQuiet("a/b.md", { "": "quiet" })).toBe(true);
    expect(isQuiet("@wt/f/a/b.md", { "@wt/f": "quiet" })).toBe(true);
    expect(isQuiet("a/b.md", { "@wt/f": "quiet" })).toBe(false);
  });

  it("does not read the prototype", () => {
    expect(isQuiet("constructor", {})).toBe(false);
    expect(isQuiet("a/__proto__", {})).toBe(false);
    expect(isQuiet("constructor/x.md", { "": "quiet" })).toBe(true);
  });
});

describe("wakingMarks", () => {
  it("names the marks whose silence ended", () => {
    const before: Marks = { a: "quiet", b: "quiet" };
    expect(wakingMarks(before, { a: "quiet" })).toEqual(["b"]);
    expect(wakingMarks(before, { a: "quiet", b: "quiet", c: "quiet" })).toEqual([]);
    expect(wakingMarks({ a: "quiet" }, { a: "quiet", "a/b": "speaking" })).toEqual(["a/b"]);
    expect(wakingMarks({}, { a: "quiet" })).toEqual([]);
  });
});

describe("SilenceStore", () => {
  it("answers for the marks it was given and tells what woke up", () => {
    const store = new SilenceStore();
    const woke: string[][] = [];
    const off = store.onWake((paths) => woke.push([...paths]));
    store.set({ docs: "quiet" });
    expect(store.isSilenced("docs/a.md")).toBe(true);
    expect(woke).toEqual([]);
    store.set({});
    expect(store.isSilenced("docs/a.md")).toBe(false);
    expect(woke).toEqual([["docs"]]);
    off();
    store.set({ x: "quiet" });
    store.set({});
    expect(woke).toHaveLength(1);
  });

  it("clears without telling anyone (the plugin went off)", () => {
    const store = new SilenceStore();
    const woke: string[][] = [];
    store.onWake((paths) => woke.push([...paths]));
    store.set({ docs: "quiet" });
    store.clear();
    expect(store.isSilenced("docs/a.md")).toBe(false);
    expect(woke).toEqual([]);
  });
});
