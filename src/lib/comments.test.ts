import { describe, expect, it } from "vitest";
import {
  barComments,
  documentTitle,
  doneComments,
  excerpt,
  hasOpenComments,
  openByDocument,
  openCount,
  trail,
} from "./comments";
import type { Comment } from "./types";

let counter = 0;
const comment = (patch: Partial<Comment> = {}): Comment => ({
  id: `n${++counter}`,
  path: "docs/plan.md",
  text: "a comment",
  quote: "a passage",
  headingPath: ["Plan"],
  line: 1,
  endLine: 1,
  createdMs: 1000 + counter,
  state: "anchored",
  done: false,
  ...patch,
});

describe("openByDocument", () => {
  it("groups the open comments by document, documents in path order and comments by line", () => {
    const list = [
      comment({ path: "docs/b.md", line: 4 }),
      comment({ path: "a.md", line: 9 }),
      comment({ path: "docs/b.md", line: 2 }),
      comment({ path: "a.md", line: 1 }),
    ];
    const groups = openByDocument(list);
    expect(groups.map((g) => g.path)).toEqual(["a.md", "docs/b.md"]);
    expect(groups[0].comments.map((n) => n.line)).toEqual([1, 9]);
    expect(groups[1].comments.map((n) => n.line)).toEqual([2, 4]);
  });

  it("leaves out the comments that are done, and a document with only those", () => {
    const groups = openByDocument([
      comment({ path: "a.md", done: true }),
      comment({ path: "b.md" }),
      comment({ path: "b.md", done: true }),
    ]);
    expect(groups.map((g) => [g.path, g.comments.length])).toEqual([["b.md", 1]]);
  });

  it("orders documents without regard to case, and keeps equal lines in the order they were written", () => {
    const first = comment({ path: "B.md", line: 3, createdMs: 5 });
    const second = comment({ path: "B.md", line: 3, createdMs: 9 });
    const groups = openByDocument([second, first, comment({ path: "a.md" })]);
    expect(groups.map((g) => g.path)).toEqual(["a.md", "B.md"]);
    expect(groups[1].comments).toEqual([first, second]);
  });

  it("does not change the list it is given", () => {
    const list = [comment({ line: 5 }), comment({ line: 1 })];
    const before = list.map((n) => n.id);
    openByDocument(list);
    expect(list.map((n) => n.id)).toEqual(before);
  });
});

describe("doneComments and openCount", () => {
  it("lists the done comments with the latest first", () => {
    const early = comment({ done: true, doneMs: 10 });
    const late = comment({ done: true, doneMs: 30 });
    const middle = comment({ done: true, doneMs: 20 });
    expect(doneComments([early, comment(), late, middle])).toEqual([late, middle, early]);
  });

  it("counts the comments that are open", () => {
    expect(openCount([])).toBe(0);
    expect(openCount([comment(), comment({ done: true }), comment()])).toBe(2);
  });
});

describe("barComments and hasOpenComments", () => {
  const list = [
    comment({ path: "a.md", line: 8 }),
    comment({ path: "a.md", line: 2 }),
    comment({ path: "a.md", state: "passageChanged" }),
    comment({ path: "a.md", done: true }),
    comment({ path: "b.md" }),
  ];

  it("gives a bar to the open comments of the document whose passage is found", () => {
    expect(barComments(list, "a.md").map((n) => n.line)).toEqual([2, 8]);
    expect(barComments(list, "b.md")).toHaveLength(1);
    expect(barComments(list, "c.md")).toEqual([]);
    expect(barComments(list, null)).toEqual([]);
  });

  it("tells whether a document has comments that are not done, found or not", () => {
    expect(hasOpenComments(list, "a.md")).toBe(true);
    expect(hasOpenComments([comment({ path: "a.md", done: true })], "a.md")).toBe(false);
    expect(hasOpenComments(list, "c.md")).toBe(false);
  });
});

describe("excerpt", () => {
  it("collapses spaces and line breaks", () => {
    expect(excerpt("  two\n\nlines   and\tgaps ")).toBe("two lines and gaps");
  });

  it("cuts a long quote at a word and says so", () => {
    const long = "word ".repeat(60);
    const cut = excerpt(long, 40);
    expect(cut.endsWith("…")).toBe(true);
    expect(cut.length).toBeLessThanOrEqual(41);
    expect(cut).toBe("word word word word word word word word…");
  });

  it("cuts in the middle of a word when there is no space to cut at", () => {
    expect(excerpt("x".repeat(100), 10)).toBe(`${"x".repeat(10)}…`);
  });

  it("keeps a short quote as it is", () => {
    expect(excerpt("short", 40)).toBe("short");
    expect(excerpt("", 40)).toBe("");
  });
});

describe("trail and documentTitle", () => {
  it("writes the headings above a passage as a trail", () => {
    expect(trail(["Plan", "Rollout"])).toBe("Plan › Rollout");
    expect(trail([])).toBe("");
  });

  it("splits a path into the name and the folder", () => {
    expect(documentTitle("docs/specs/plan.md")).toEqual({ name: "plan.md", folder: "docs/specs" });
    expect(documentTitle("README.md")).toEqual({ name: "README.md", folder: "" });
  });
});
