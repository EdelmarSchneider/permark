import { describe, expect, it } from "vitest";
import type { CommentView, OutlineItem } from "../../api";
import {
  QUOTE_CHARS,
  agentCommentPrompt,
  agentCommentsPrompt,
  barComments,
  doneComments,
  documentTitle,
  excerpt,
  headingPathAt,
  openByDocument,
  openCounts,
  touchedBlocks,
  trail,
} from "./model";

function comment(over: Partial<CommentView>): CommentView {
  return {
    id: "c1",
    path: "docs/a.md",
    text: "Say it better",
    quote: "the quoted passage",
    headingPath: ["Plan", "Rollout"],
    line: 12,
    endLine: 12,
    createdMs: 1000,
    state: "anchored",
    done: false,
    doneMs: null,
    doneBy: null,
    reply: null,
    replies: [],
    ...over,
  };
}

describe("the lists", () => {
  const all = [
    comment({ id: "a", path: "Z.md", line: 3 }),
    comment({ id: "b", path: "a.md", line: 9, createdMs: 5 }),
    comment({ id: "c", path: "a.md", line: 2, createdMs: 9 }),
    comment({ id: "d", path: "a.md", line: 2, createdMs: 4 }),
    comment({ id: "e", path: "a.md", done: true, doneMs: 50 }),
    comment({ id: "f", path: "b.md", done: true, doneMs: 90 }),
  ];

  it("groups the open ones by document (path order, no case) and by line", () => {
    const groups = openByDocument(all);
    expect(groups.map((g) => g.path)).toEqual(["a.md", "Z.md"]);
    expect(groups[0]?.comments.map((c) => c.id)).toEqual(["d", "c", "b"]);
  });

  it("lists the done ones latest first", () => {
    expect(doneComments(all).map((c) => c.id)).toEqual(["f", "e"]);
  });

  it("counts the open ones of each document", () => {
    expect([...openCounts(all)]).toEqual([
      ["Z.md", 1],
      ["a.md", 3],
    ]);
  });

  it("gives a bar only to open comments whose passage was found", () => {
    const some = [
      comment({ id: "1", state: "anchored" }),
      comment({ id: "2", state: "moved", line: 3 }),
      comment({ id: "3", state: "passageChanged" }),
      comment({ id: "4", state: "documentMissing" }),
      comment({ id: "5", done: true }),
      comment({ id: "6", path: "other.md" }),
    ];
    expect(barComments(some, "docs/a.md").map((c) => c.id)).toEqual(["2", "1"]);
    expect(barComments(some, null)).toEqual([]);
  });
});

describe("words", () => {
  it("shortens a quote at a word, with an ellipsis", () => {
    expect(excerpt("  a  b \n c ")).toBe("a b c");
    const long = "word ".repeat(100);
    const cut = excerpt(long, 40);
    expect(cut.length).toBeLessThanOrEqual(41);
    expect(cut.endsWith("…")).toBe(true);
    expect(cut.endsWith(" …")).toBe(false);
    expect(excerpt("x".repeat(300), 20)).toBe(`${"x".repeat(20)}…`);
  });

  it("writes a trail and a title", () => {
    expect(trail(["Plan", "Rollout"])).toBe("Plan › Rollout");
    expect(trail([])).toBe("");
    expect(documentTitle("docs/specs/a.md")).toEqual({ name: "a.md", folder: "docs/specs" });
    expect(documentTitle("a.md")).toEqual({ name: "a.md", folder: "" });
  });
});

describe("the page", () => {
  const blocks = [
    { id: "h1", lineStart: 1, lineEnd: 1 },
    { id: "p1", lineStart: 3, lineEnd: 4 },
    { id: "h2", lineStart: 6, lineEnd: 6 },
    { id: "p2", lineStart: 8, lineEnd: 12 },
    { id: "p3", lineStart: 14, lineEnd: 14 },
  ];

  it("finds the blocks a passage touches", () => {
    expect(touchedBlocks(blocks, 3, 3)).toEqual(["p1"]);
    expect(touchedBlocks(blocks, 4, 9)).toEqual(["p1", "h2", "p2"]);
    expect(touchedBlocks(blocks, 2, 2)).toEqual([]);
    expect(touchedBlocks(blocks, 12, 3)).toEqual(["p1", "h2", "p2"]);
    expect(touchedBlocks([{ id: "x", lineStart: 0, lineEnd: 0 }], 1, 1)).toEqual([]);
  });

  it("names the headings above a block", () => {
    const outline: OutlineItem[] = [
      { label: "Plan", level: 1, id: "plan", line: 1, block: "h1" },
      { label: "Rollout", level: 2, id: "rollout", line: 6, block: "h2" },
    ];
    expect(headingPathAt(blocks, outline, "p1")).toEqual(["Plan"]);
    expect(headingPathAt(blocks, outline, "p2")).toEqual(["Plan", "Rollout"]);
    expect(headingPathAt(blocks, outline, "h2")).toEqual(["Plan", "Rollout"]);
    expect(headingPathAt(blocks, outline, "nope")).toEqual([]);
    expect(headingPathAt(blocks, [], "p1")).toEqual([]);
  });
});

describe("the prompt for an agent", () => {
  it("names the document, the place, the quote and the text", () => {
    const text = agentCommentPrompt(comment({}));
    expect(text).toContain("Please address this comment on docs/a.md (Plan › Rollout, line 12):");
    expect(text).toContain("> the quoted passage");
    expect(text).toContain("Say it better");
    expect(text).toContain("`resolve_comment`");
  });

  it("quotes a long passage up to its limit", () => {
    const text = agentCommentPrompt(comment({ quote: "word ".repeat(1000) }));
    const quoted = text.split("\n").find((l) => l.startsWith("> ")) ?? "";
    expect(quoted.length).toBeLessThanOrEqual(QUOTE_CHARS + 3);
  });

  it("groups several comments by document, numbered", () => {
    const text = agentCommentsPrompt(
      openByDocument([
        comment({ id: "1", path: "b.md", text: "first", line: 4, headingPath: [] }),
        comment({ id: "2", path: "a.md", text: "second", line: 2 }),
        comment({ id: "3", path: "a.md", text: "third", line: 8 }),
      ]),
    );
    expect(text.indexOf("## a.md")).toBeLessThan(text.indexOf("## b.md"));
    expect(text).toContain("1. Plan › Rollout, line 2:");
    expect(text).toContain("2. Plan › Rollout, line 8:");
    expect(text).toContain("1. line 4:");
    expect(agentCommentsPrompt([])).toBe("");
  });
});
