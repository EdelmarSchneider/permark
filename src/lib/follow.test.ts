import { describe, expect, it } from "vitest";
import { FollowGate, followCandidate } from "./follow";
import type { FsBatch } from "./types";

const batch = (changes: FsBatch["changes"], bulk: FsBatch["bulk"] = null): FsBatch => ({
  changes,
  bulk,
  rescan: false,
});

describe("followCandidate", () => {
  it("picks the latest changed Markdown document", () => {
    const b = batch([
      { kind: "modified", path: "a.md", atMs: 10 },
      { kind: "created", path: "b.md", atMs: 30 },
      { kind: "modified", path: "c.md", atMs: 20 },
    ]);
    expect(followCandidate(b)).toBe("b.md");
  });

  it("ignores non-Markdown files and removals", () => {
    const b = batch([
      { kind: "modified", path: "data.json", atMs: 50 },
      { kind: "removed", path: "gone.md", atMs: 40 },
      { kind: "modified", path: "a.md", atMs: 10 },
    ]);
    expect(followCandidate(b)).toBe("a.md");
    expect(followCandidate(batch([{ kind: "modified", path: "x.rs", atMs: 1 }]))).toBeNull();
  });

  it("follows the new path of a rename", () => {
    const b = batch([{ kind: "renamed", path: "new.md", from: "old.md", atMs: 5 }]);
    expect(followCandidate(b)).toBe("new.md");
  });

  it("never follows a bulk batch", () => {
    const b = batch([{ kind: "modified", path: "a.md", atMs: 1 }], {
      count: 143,
      reason: "gitOperation",
      paths: ["a.md"],
    });
    expect(followCandidate(b)).toBeNull();
  });
});

describe("FollowGate", () => {
  it("allows the first jump and rate-limits the next ones to one per dwell", () => {
    const gate = new FollowGate(3000);
    expect(gate.offer("a.md", 1000, false)).toBe("a.md");
    expect(gate.offer("b.md", 2000, false)).toBeNull();
    expect(gate.msUntilDue(2000)).toBe(2000);
    expect(gate.due(3999, false)).toBeNull();
    expect(gate.due(4000, false)).toBe("b.md");
    expect(gate.msUntilDue(4000)).toBeNull();
  });

  it("keeps only the latest deferred offer", () => {
    const gate = new FollowGate(3000);
    gate.offer("a.md", 0, false);
    gate.offer("b.md", 500, false);
    gate.offer("c.md", 900, false);
    expect(gate.due(3000, false)).toBe("c.md");
  });

  it("drops offers while blocked (selection or overlay open)", () => {
    const gate = new FollowGate(3000);
    expect(gate.offer("a.md", 10_000, true)).toBeNull();
    expect(gate.msUntilDue(10_000)).toBeNull();
    expect(gate.offer("a.md", 10_001, false)).toBe("a.md");
  });

  it("drops a deferred offer when it comes due while blocked", () => {
    const gate = new FollowGate(3000);
    gate.offer("a.md", 0, false);
    gate.offer("b.md", 100, false);
    expect(gate.due(3200, true)).toBeNull();
    expect(gate.due(3300, false)).toBeNull();
  });

  it("allows another jump a full dwell after the last one", () => {
    const gate = new FollowGate(3000);
    gate.offer("a.md", 0, false);
    expect(gate.offer("b.md", 3000, false)).toBe("b.md");
  });
});
