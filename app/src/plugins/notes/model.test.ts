import { describe, expect, it } from "vitest";
import type { NoteEntry } from "../../api";
import {
  arrange,
  dropPinned,
  drawnOrder,
  filterNotes,
  foldChars,
  groupOf,
  marksOf,
  matchesFilter,
  movePinned,
  neighbour,
  pinnedAt,
  placeOf,
  shortFolder,
  shownName,
} from "./model";

const NOW = new Date(2026, 9, 19, 15, 0).getTime();
const at = (days: number, hour = 10): number => new Date(2026, 9, 19 - days, hour, 0).getTime();

function entry(over: Partial<NoteEntry> & { path: string }): NoteEntry {
  return {
    kind: "note",
    name: over.path,
    title: over.path,
    snippet: "",
    modifiedMs: NOW,
    size: 1,
    pinned: false,
    shared: false,
    copied: false,
    recovered: false,
    folder: null,
    state: null,
    pending: false,
    ...over,
  };
}

describe("groupOf", () => {
  it("counts calendar days on the person's clock", () => {
    expect(groupOf(at(0, 0), NOW)).toBe("today");
    expect(groupOf(NOW + 5 * 60_000, NOW)).toBe("today");
    expect(groupOf(at(1, 23), NOW)).toBe("yesterday");
    expect(groupOf(at(2), NOW)).toBe("week");
    expect(groupOf(at(6), NOW)).toBe("week");
    expect(groupOf(at(7), NOW)).toBe("month");
    expect(groupOf(at(18), NOW)).toBe("month");
    expect(groupOf(at(19), NOW)).toBe("older");
    expect(groupOf(new Date(2025, 9, 19).getTime(), NOW)).toBe("older");
  });
});

describe("arrange", () => {
  const notes = [
    entry({ path: "old.md", modifiedMs: at(40) }),
    entry({ path: "today.md", modifiedMs: at(0) }),
    entry({ path: "pin-b.md", pinned: true, modifiedMs: at(30) }),
    entry({ path: "pin-a.md", pinned: true, modifiedMs: at(0) }),
    entry({ path: "yday.md", modifiedMs: at(1) }),
    entry({ path: "@file/0a1b2c3d/x.md", kind: "file" }),
    entry({ path: "@file/0a1b2c3d/pinned.md", kind: "file", pinned: true }),
  ];

  it("puts pinned first in the order given, then groups newest first, then the files", () => {
    const arranged = arrange(notes, NOW);
    expect(arranged.pinned.map((n) => n.path)).toEqual([
      "pin-b.md",
      "pin-a.md",
      "@file/0a1b2c3d/pinned.md",
    ]);
    expect(arranged.groups.map((g) => g.id)).toEqual(["today", "yesterday", "older"]);
    expect(arranged.files.map((n) => n.path)).toEqual(["@file/0a1b2c3d/x.md"]);
    expect(drawnOrder(arranged).map((n) => n.path)).toEqual([
      "pin-b.md",
      "pin-a.md",
      "@file/0a1b2c3d/pinned.md",
      "today.md",
      "yday.md",
      "old.md",
      "@file/0a1b2c3d/x.md",
    ]);
  });

  it("keeps the note being edited where it was until it is left", () => {
    const edited = [
      entry({ path: "a.md", modifiedMs: at(1) }),
      entry({ path: "b.md", modifiedMs: NOW - 1000 }),
    ];
    const frozen = { path: "a.md", modifiedMs: at(1) };
    // "a.md" was saved just now, which would put it first today
    const typed = [entry({ path: "a.md", modifiedMs: NOW }), edited[1] as NoteEntry];
    expect(arrange(typed, NOW).groups[0]?.notes.map((n) => n.path)).toEqual(["a.md", "b.md"]);
    const stays = arrange(typed, NOW, frozen);
    expect(stays.groups.map((g) => [g.id, g.notes.map((n) => n.path)])).toEqual([
      ["today", ["b.md"]],
      ["yesterday", ["a.md"]],
    ]);
  });

  it("breaks ties by name so the order does not flicker", () => {
    const same = [entry({ path: "b.md" }), entry({ path: "a.md" })];
    expect(arrange(same, NOW).groups[0]?.notes.map((n) => n.path)).toEqual(["a.md", "b.md"]);
  });

  it("is empty for nothing", () => {
    expect(arrange([], NOW)).toEqual({ pinned: [], groups: [], files: [] });
  });
});

describe("filter", () => {
  const note = entry({
    path: "x.md",
    title: "Café plans",
    snippet: "Numbers for the SÃO PAULO trip",
    folder: "~\\Downloads",
  });
  it("needs every word, ignoring case and accents", () => {
    expect(matchesFilter(note, "")).toBe(true);
    expect(matchesFilter(note, "  ")).toBe(true);
    expect(matchesFilter(note, "cafe")).toBe(true);
    expect(matchesFilter(note, "CAFÉ trip")).toBe(true);
    expect(matchesFilter(note, "sao paulo")).toBe(true);
    expect(matchesFilter(note, "downloads")).toBe(true);
    expect(matchesFilter(note, "cafe tokyo")).toBe(false);
  });
  it("keeps the order", () => {
    const list = [entry({ path: "a.md", title: "alpha" }), entry({ path: "b.md", title: "beta" })];
    expect(filterNotes(list, "a").map((n) => n.path)).toEqual(["a.md", "b.md"]);
    expect(filterNotes(list, "alp").map((n) => n.path)).toEqual(["a.md"]);
  });
  it("folds accents", () => {
    expect(foldChars("Àéîõü Ç")).toBe("aeiou c");
  });
});

describe("pin order", () => {
  const order = ["a", "b", "c"];
  it("moves one place and stops at the ends", () => {
    expect(movePinned(order, "b", -1)).toEqual(["b", "a", "c"]);
    expect(movePinned(order, "b", 1)).toEqual(["a", "c", "b"]);
    expect(movePinned(order, "a", -1)).toEqual(order);
    expect(movePinned(order, "c", 1)).toEqual(order);
    expect(movePinned(order, "zz", 1)).toEqual(order);
  });
  it("drops on a target", () => {
    expect(dropPinned(order, "c", "a")).toEqual(["c", "a", "b"]);
    expect(dropPinned(order, "a", "c")).toEqual(["b", "c", "a"]);
    expect(dropPinned(order, "a", "a")).toEqual(order);
    expect(dropPinned(order, "x", "a")).toEqual(order);
  });
  it("jumps to the nth pinned note for Mod+1..9", () => {
    const pinned = [entry({ path: "a" }), entry({ path: "b" })];
    expect(pinnedAt(pinned, 1)?.path).toBe("a");
    expect(pinnedAt(pinned, 2)?.path).toBe("b");
    expect(pinnedAt(pinned, 3)).toBeNull();
    expect(pinnedAt(pinned, 0)).toBeNull();
    expect(pinnedAt(pinned, 10)).toBeNull();
  });
});

describe("neighbour", () => {
  it("steps and wraps", () => {
    const shown = ["a", "b", "c"];
    expect(neighbour(shown, "a", 1)).toBe("b");
    expect(neighbour(shown, "c", 1)).toBe("a");
    expect(neighbour(shown, "a", -1)).toBe("c");
    expect(neighbour(shown, null, 1)).toBe("a");
    expect(neighbour(shown, null, -1)).toBe("c");
    expect(neighbour(shown, "gone", 1)).toBe("a");
    expect(neighbour([], "a", 1)).toBeNull();
  });
});

describe("names and places", () => {
  it("splits a path of either kind", () => {
    expect(placeOf("C:\\work\\plan\\ask.md")).toEqual({ file: "ask.md", folder: "plan" });
    expect(placeOf("/a/b/c.md")).toEqual({ file: "c.md", folder: "b" });
    expect(placeOf("c.md")).toEqual({ file: "c.md", folder: "" });
  });
  it("shows the title, else the file name without its extension", () => {
    expect(shownName({ title: "Plan", name: "x.md" })).toBe("Plan");
    expect(shownName({ title: "", name: "2026-10-19 0907.md" })).toBe("2026-10-19 0907");
    expect(shownName({ title: "", name: ".hidden" })).toBe(".hidden");
  });
  it("cuts a long folder in the middle", () => {
    expect(shortFolder("~\\a")).toBe("~\\a");
    const long = "C:\\Users\\someone\\Documents\\projects\\clients\\acme\\contracts\\2026";
    const cut = shortFolder(long);
    expect(cut.length).toBeLessThanOrEqual(34);
    expect(cut.startsWith("C:\\…\\")).toBe(true);
    expect(cut.endsWith("2026")).toBe(true);
    expect(shortFolder("/a/b")).toBe("/a/b");
    expect(shortFolder(`x${"y".repeat(60)}`).startsWith("…")).toBe(true);
  });
});

describe("marks", () => {
  it("reads the row's state", () => {
    const marks = marksOf(
      entry({ path: "a.md", recovered: true, shared: true, copied: true, state: "unsaved" }),
    );
    expect(marks).toEqual({
      recovered: true,
      unsaved: true,
      changed: false,
      gone: false,
      shared: true,
      copied: true,
    });
    expect(marksOf(entry({ path: "b.md", state: "gone" })).gone).toBe(true);
    expect(marksOf(entry({ path: "b.md", state: "changedOnDisk" })).changed).toBe(true);
  });
});
