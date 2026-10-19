// @vitest-environment jsdom
import "../../core/testing/media";
import { describe, expect, it } from "vitest";
import type { Block, ReviewChange, ReviewView } from "../../api";
import { barHtml, hashOf, kindWord, reviewBlocks } from "./blocks";

const block = (id: string, html = `<p data-b="${id}">${id}</p>`): Block => ({
  id,
  kind: "p",
  lineStart: 1,
  lineEnd: 1,
  html,
});

function change(
  index: number,
  start: number,
  len: number,
  kind: ReviewChange["kind"],
  ghosts: Block[],
): ReviewChange {
  return { index, kind, start, len, ghosts };
}

function view(changes: ReviewChange[], canKeep = true): ReviewView {
  return {
    path: "a.md",
    hash: "h",
    basis: "read",
    asked: "read",
    count: changes.length,
    canKeep,
    changes,
  };
}

describe("the page in review", () => {
  const blocks = [block("a"), block("b"), block("c")];

  it("puts the ghosts of what went and the bar of each change before the blocks it covers", () => {
    const shown = reviewBlocks(
      blocks,
      view([
        change(0, 1, 1, "edited", [block("old")]),
        change(1, 3, 0, "removed", [block("gone")]),
      ]),
      0,
    );
    expect(shown.map((b) => b.id.split(":")[1]?.replace(/[0-9a-f]{3,8}$/, "") ?? b.id)).toEqual([
      "a",
      "ghost0.0",
      "bar0",
      "b",
      "c",
      "ghost1.0",
      "bar1",
    ]);
    const ghost = shown[1];
    expect(ghost?.html).toContain('class="rv-ghost"');
    expect(ghost?.html).not.toContain("data-b=");
    expect(ghost?.html).toContain("old");
    expect(shown[2]?.html).toContain('data-rv-act="revert"');
    expect(shown[2]?.html).toContain('data-rv-act="keep"');
  });

  it("offers no Keep when the base is a commit, and says which change is the current one", () => {
    const only = view([change(0, 0, 1, "added", [])], false);
    const bar = barHtml(only.changes[0] as ReviewChange, 1, false, true);
    expect(bar).not.toContain('data-rv-act="keep"');
    expect(bar).toContain("is-current");
    expect(barHtml(only.changes[0] as ReviewChange, 1, true, false)).not.toContain("is-current");
  });

  it("a synthetic block has the same id for the same look and another when the look changes", () => {
    const one = reviewBlocks(blocks, view([change(0, 1, 1, "edited", [block("old")])]), 0);
    const again = reviewBlocks(blocks, view([change(0, 1, 1, "edited", [block("old")])]), 0);
    expect(again.map((b) => b.id)).toEqual(one.map((b) => b.id));
    const moved = reviewBlocks(blocks, view([change(0, 1, 1, "edited", [block("old")])]), -1);
    expect(moved.map((b) => b.id)).not.toEqual(one.map((b) => b.id));
    expect(hashOf("x")).toBe(hashOf("x"));
    expect(hashOf("x")).not.toBe(hashOf("y"));
  });

  it("leaves the blocks alone when there is nothing to show", () => {
    expect(reviewBlocks(blocks, view([]), -1)).toEqual(blocks);
    expect(kindWord("edited")).toMatch(/modif|chang|edit/i);
  });
});
