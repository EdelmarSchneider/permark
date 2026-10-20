import { describe, expect, it } from "vitest";
import {
  FAR_LINES,
  MAX_HELD_CHARS,
  append,
  linesOf,
  loading,
  megabytes,
  start,
  wantsMore,
} from "./largeText";

const owner = {};

describe("the lines of a large text", () => {
  it("are the lines of the text, with no empty line after the last break", () => {
    expect(linesOf("a\nb\n")).toEqual({ lines: ["a", "b"], open: false });
    expect(linesOf("a\nb")).toEqual({ lines: ["a", "b"], open: true });
    expect(linesOf("")).toEqual({ lines: [], open: false });
    expect(linesOf("\n\n")).toEqual({ lines: ["", ""], open: false });
    // a carriage return stays with its line (the viewer drops it when it draws)
    expect(linesOf("a\r\nb\r\n").lines).toEqual(["a\r", "b\r"]);
  });

  it("start with the first part, and say where the rest begins", () => {
    const held = start(owner, "one\ntwo\n", 8);
    expect(held).toMatchObject({ lines: ["one", "two"], open: false, chars: 8, next: 8 });
    expect(held.load).toEqual({ status: "idle" });
    // a whole file has nothing to load
    expect(start(owner, "x\n", null).load).toEqual({ status: "done" });
  });

  it("grow by chunks, and a line cut between two chunks is one line again", () => {
    let held = start(owner, "alpha\nbet", 9);
    expect(held.open).toBe(true);
    held = append(held, { text: "a\ngamma\ndel", next: 20 });
    expect(held.lines).toEqual(["alpha", "beta", "gamma", "del"]);
    expect(held.open).toBe(true);
    held = append(held, { text: "ta\n", next: null });
    expect(held.lines).toEqual(["alpha", "beta", "gamma", "delta"]);
    expect(held.open).toBe(false);
    expect(held.load).toEqual({ status: "done" });
    expect(held.next).toBeNull();
    expect(held.chars).toBe(9 + 11 + 3);
  });

  it("go on from an empty chunk, and from a chunk with no break in it", () => {
    let held = start(owner, "a\n", 2);
    held = append(held, { text: "", next: 2 });
    expect(held.lines).toEqual(["a"]);
    held = append(held, { text: "no break", next: 10 });
    expect(held.lines).toEqual(["a", "no break"]);
    held = append(held, { text: " yet", next: null });
    expect(held.lines).toEqual(["a", "no break yet"]);
  });

  it("stop asking when enough is held", () => {
    const held = { ...start(owner, "a\n", 2), chars: MAX_HELD_CHARS - 1 };
    expect(append(held, { text: "bb\n", next: 5 }).load).toEqual({ status: "capped" });
    expect(append(held, { text: "bb\n", next: null }).load).toEqual({ status: "done" });
  });

  it("want the next chunk only near the end, one at a time", () => {
    const lines = Array.from({ length: FAR_LINES * 3 }, (_, i) => `l${i}`).join("\n") + "\n";
    const held = start(owner, lines, 999);
    const total = FAR_LINES * 3;
    expect(wantsMore(held, 0, 100)).toBe(false);
    expect(wantsMore(held, total - FAR_LINES - 100, 100)).toBe(true);
    expect(wantsMore(loading(held, { status: "loading" }), total, 100)).toBe(false);
    expect(wantsMore(loading(held, { status: "failed" }), total, 100)).toBe(false);
    expect(wantsMore(start(owner, lines, null), total, 100)).toBe(false);
  });

  it("round sizes up to a whole megabyte, never to nothing", () => {
    expect(megabytes(10)).toBe(1);
    expect(megabytes(310 * 1024 * 1024)).toBe(310);
    expect(megabytes(1.5 * 1024 * 1024)).toBe(2);
  });
});
