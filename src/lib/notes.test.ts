import { describe, expect, it } from "vitest";
import {
  barNotes,
  documentTitle,
  doneNotes,
  excerpt,
  hasOpenNotes,
  openByDocument,
  openCount,
  trail,
} from "./notes";
import type { Note } from "./types";

let counter = 0;
const note = (patch: Partial<Note> = {}): Note => ({
  id: `n${++counter}`,
  path: "docs/plan.md",
  text: "a note",
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
  it("groups the open notes by document, documents in path order and notes by line", () => {
    const list = [
      note({ path: "docs/b.md", line: 4 }),
      note({ path: "a.md", line: 9 }),
      note({ path: "docs/b.md", line: 2 }),
      note({ path: "a.md", line: 1 }),
    ];
    const groups = openByDocument(list);
    expect(groups.map((g) => g.path)).toEqual(["a.md", "docs/b.md"]);
    expect(groups[0].notes.map((n) => n.line)).toEqual([1, 9]);
    expect(groups[1].notes.map((n) => n.line)).toEqual([2, 4]);
  });

  it("leaves out the notes that are done, and a document with only those", () => {
    const groups = openByDocument([
      note({ path: "a.md", done: true }),
      note({ path: "b.md" }),
      note({ path: "b.md", done: true }),
    ]);
    expect(groups.map((g) => [g.path, g.notes.length])).toEqual([["b.md", 1]]);
  });

  it("orders documents without regard to case, and keeps equal lines in the order they were written", () => {
    const first = note({ path: "B.md", line: 3, createdMs: 5 });
    const second = note({ path: "B.md", line: 3, createdMs: 9 });
    const groups = openByDocument([second, first, note({ path: "a.md" })]);
    expect(groups.map((g) => g.path)).toEqual(["a.md", "B.md"]);
    expect(groups[1].notes).toEqual([first, second]);
  });

  it("does not change the list it is given", () => {
    const list = [note({ line: 5 }), note({ line: 1 })];
    const before = list.map((n) => n.id);
    openByDocument(list);
    expect(list.map((n) => n.id)).toEqual(before);
  });
});

describe("doneNotes and openCount", () => {
  it("lists the done notes with the latest first", () => {
    const early = note({ done: true, doneMs: 10 });
    const late = note({ done: true, doneMs: 30 });
    const middle = note({ done: true, doneMs: 20 });
    expect(doneNotes([early, note(), late, middle])).toEqual([late, middle, early]);
  });

  it("counts the notes that are open", () => {
    expect(openCount([])).toBe(0);
    expect(openCount([note(), note({ done: true }), note()])).toBe(2);
  });
});

describe("barNotes and hasOpenNotes", () => {
  const list = [
    note({ path: "a.md", line: 8 }),
    note({ path: "a.md", line: 2 }),
    note({ path: "a.md", state: "passageChanged" }),
    note({ path: "a.md", done: true }),
    note({ path: "b.md" }),
  ];

  it("gives a bar to the open notes of the document whose passage is found", () => {
    expect(barNotes(list, "a.md").map((n) => n.line)).toEqual([2, 8]);
    expect(barNotes(list, "b.md")).toHaveLength(1);
    expect(barNotes(list, "c.md")).toEqual([]);
    expect(barNotes(list, null)).toEqual([]);
  });

  it("tells whether a document has notes that are not done, found or not", () => {
    expect(hasOpenNotes(list, "a.md")).toBe(true);
    expect(hasOpenNotes([note({ path: "a.md", done: true })], "a.md")).toBe(false);
    expect(hasOpenNotes(list, "c.md")).toBe(false);
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
