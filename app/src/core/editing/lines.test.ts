import { describe, expect, it } from "vitest";
import { MAX_D, applyEdits, diff, lineEdits } from "./lines";

/** Small deterministic random (tests repeat). */
function rng(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 2 ** 32;
  };
}

describe("lineEdits", () => {
  it("is empty for equal texts and sends only changed lines", () => {
    expect(lineEdits("a\nb\n", "a\nb\n")).toEqual([]);
    expect(lineEdits("a\nb\nc\n", "a\nB\nc\n")).toEqual([{ at: 1, remove: 1, insert: ["B"] }]);
  });

  it("keeps untouched lines between two edits", () => {
    const base = Array.from({ length: 50 }, (_, i) => `line ${i}`).join("\n");
    const lines = base.split("\n");
    lines[3] = "changed 3";
    lines[40] = "changed 40";
    const edits = lineEdits(base, lines.join("\n"));
    expect(edits).toEqual([
      { at: 3, remove: 1, insert: ["changed 3"] },
      { at: 40, remove: 1, insert: ["changed 40"] },
    ]);
  });

  it("handles insertions, deletions, the final newline and empty texts", () => {
    expect(lineEdits("a\nb", "a\nb\n")).toEqual([{ at: 2, remove: 0, insert: [""] }]);
    expect(lineEdits("", "x")).toEqual([{ at: 0, remove: 1, insert: ["x"] }]);
    expect(applyEdits("a\nb\nc", lineEdits("a\nb\nc", "c"))).toBe("c");
    expect(applyEdits("x", lineEdits("x", ""))).toBe("");
  });

  it("gives back the new text for random edits (property)", () => {
    const next = rng(7);
    for (let round = 0; round < 300; round++) {
      const base = Array.from({ length: Math.floor(next() * 30) }, () =>
        String.fromCharCode(97 + Math.floor(next() * 4)),
      );
      const changed = [...base];
      const steps = Math.floor(next() * 6);
      for (let s = 0; s < steps; s++) {
        const at = Math.floor(next() * (changed.length + 1));
        const r = next();
        if (r < 0.4) changed.splice(at, 0, "new" + s);
        else if (r < 0.7) changed.splice(at, 1);
        else if (at < changed.length) changed[at] = "mod" + s;
      }
      const a = base.join("\n");
      const b = changed.join("\n");
      const edits = lineEdits(a, b);
      expect(applyEdits(a, edits)).toBe(b);
      // edits are sorted and do not overlap
      let cursor = 0;
      for (const edit of edits) {
        expect(edit.at).toBeGreaterThanOrEqual(cursor);
        cursor = edit.at + edit.remove;
      }
    }
  });

  it("finds a minimal script for small changes and stays correct past the limit", () => {
    const a = Array.from({ length: 5000 }, (_, i) => `a${i}`);
    const b = Array.from({ length: 5000 }, (_, i) => `b${i}`);
    const ops = diff(a, b);
    expect(ops).toEqual([{ kind: "change", a: 0, remove: 5000, b: 0, add: 5000 }]);
    expect(MAX_D).toBeGreaterThan(100);
    const started = performance.now();
    const big = Array.from({ length: 30000 }, (_, i) => `line ${i}`);
    const edited = [...big];
    edited[15000] = "x";
    expect(lineEdits(big.join("\n"), edited.join("\n"))).toEqual([
      { at: 15000, remove: 1, insert: ["x"] },
    ]);
    expect(performance.now() - started).toBeLessThan(2000);
  });
});
