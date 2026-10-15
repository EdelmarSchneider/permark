import { describe, expect, it } from "vitest";
import {
  arrange,
  drawnOrder,
  dropPinned,
  filterNotes,
  groupOf,
  matchesFilter,
  movePinned,
  neighbour,
  pinnedAt,
  fullPlace,
  placeOf,
  saveStatus,
  shortFolder,
} from "./notesModel";
import type { NoteEntry } from "./types";

// the clock is given as local dates, so these hold in any time zone
const at = (y: number, m: number, d: number, h = 12, min = 0): number =>
  new Date(y, m - 1, d, h, min).getTime();
/** Thursday 15 October 2026, 14:32. */
const NOW = at(2026, 10, 15, 14, 32);

const note = (path: string, modifiedMs: number, extra: Partial<NoteEntry> = {}): NoteEntry => ({
  kind: "note",
  path,
  title: path.replace(/\.md$/, ""),
  snippet: "",
  modifiedMs,
  sizeBytes: 10,
  pinned: false,
  copied: false,
  view: null,
  folder: null,
  missing: false,
  unsaved: false,
  shared: false,
  ...extra,
});

describe("groupOf", () => {
  it("groups by the calendar of the person's clock", () => {
    expect(groupOf(at(2026, 10, 15, 0, 0), NOW)).toBe("today");
    expect(groupOf(at(2026, 10, 15, 14, 32), NOW)).toBe("today");
    expect(groupOf(at(2026, 10, 14, 23, 59), NOW)).toBe("yesterday");
    expect(groupOf(at(2026, 10, 14, 0, 0), NOW)).toBe("yesterday");
    expect(groupOf(at(2026, 10, 13), NOW)).toBe("week");
    expect(groupOf(at(2026, 10, 9), NOW)).toBe("week");
    expect(groupOf(at(2026, 10, 8), NOW)).toBe("month");
    expect(groupOf(at(2026, 10, 1), NOW)).toBe("month");
    expect(groupOf(at(2026, 9, 30), NOW)).toBe("older");
    expect(groupOf(at(2025, 10, 15), NOW)).toBe("older");
  });

  it("puts a note from the last week in the week even when it is from the month before", () => {
    const early = at(2026, 11, 2);
    expect(groupOf(at(2026, 10, 30), early)).toBe("week");
    expect(groupOf(at(2026, 10, 20), early)).toBe("older");
  });

  it("treats a note from the future (a clock that was wrong) as today", () => {
    expect(groupOf(at(2026, 10, 16), NOW)).toBe("today");
  });
});

describe("arrange", () => {
  const entries = [
    note("old.md", at(2026, 8, 1)),
    note("today-early.md", at(2026, 10, 15, 8)),
    note("sticky-b.md", at(2026, 3, 1), { pinned: true }),
    note("yesterday.md", at(2026, 10, 14, 18)),
    note("today-late.md", at(2026, 10, 15, 14)),
    note("sticky-a.md", at(2026, 4, 1), { pinned: true }),
    note("week.md", at(2026, 10, 12)),
  ];

  it("lists the pinned notes first, as given, then the groups with the newest first", () => {
    const { pinned, groups } = arrange(entries, NOW);
    expect(pinned.map((n) => n.path)).toEqual(["sticky-b.md", "sticky-a.md"]);
    expect(groups.map((g) => g.id)).toEqual(["today", "yesterday", "week", "older"]);
    expect(groups[0].notes.map((n) => n.path)).toEqual(["today-late.md", "today-early.md"]);
    expect(groups[3].notes.map((n) => n.path)).toEqual(["old.md"]);
  });

  it("leaves out the groups with nothing in them", () => {
    expect(arrange([], NOW)).toEqual({ pinned: [], groups: [], files: [] });
  });

  it("puts the loose files after the groups, as given, and the pinned ones among the pinned rows", () => {
    const file = (path: string, extra: Partial<NoteEntry> = {}): NoteEntry =>
      note(path, 0, { kind: "file", title: path, folder: "~\\Downloads", ...extra });
    const mixed = [
      file("@file/00000001/opened-last.md"),
      note("today.md", at(2026, 10, 15, 8)),
      file("@file/00000002/sticky.md", { pinned: true }),
      file("@file/00000003/opened-first.md"),
      note("sticky-note.md", at(2026, 3, 1), { pinned: true }),
    ];
    const arranged = arrange(mixed, NOW);
    expect(arranged.pinned.map((n) => n.path)).toEqual([
      "@file/00000002/sticky.md",
      "sticky-note.md",
    ]);
    expect(arranged.groups.map((g) => g.notes.map((n) => n.path))).toEqual([["today.md"]]);
    expect(arranged.files.map((n) => n.path)).toEqual([
      "@file/00000001/opened-last.md",
      "@file/00000003/opened-first.md",
    ]);
    expect(drawnOrder(arranged).map((n) => n.path)).toEqual([
      "@file/00000002/sticky.md",
      "sticky-note.md",
      "today.md",
      "@file/00000001/opened-last.md",
      "@file/00000003/opened-first.md",
    ]);
    // a file's time of last change does not group it
    expect(arrange([file("@file/00000004/x.md", { modifiedMs: NOW })], NOW).groups).toEqual([]);
  });

  it("keeps the note being edited where it was until it is left", () => {
    // 'old.md' is being edited: its modification time is now, but it stays in Older
    const typing = entries.map((n) => (n.path === "old.md" ? { ...n, modifiedMs: NOW } : n));
    const frozen = { path: "old.md", modifiedMs: at(2026, 8, 1) };
    const staying = arrange(typing, NOW, frozen);
    expect(staying.groups.map((g) => g.id)).toEqual(["today", "yesterday", "week", "older"]);
    expect(staying.groups[3].notes.map((n) => n.path)).toEqual(["old.md"]);
    // left: it moves to the top of Today
    const left = arrange(typing, NOW, null);
    expect(left.groups[0].notes[0].path).toBe("old.md");
    expect(left.groups.map((g) => g.id)).not.toContain("older");
  });

  it("orders notes edited at the same moment by name", () => {
    const same = [note("b.md", NOW), note("a.md", NOW)];
    expect(arrange(same, NOW).groups[0].notes.map((n) => n.path)).toEqual(["a.md", "b.md"]);
  });
});

describe("the filter", () => {
  it("finds a loose file by its name or by the folder it is in", () => {
    const file = note("@file/0a1b2c3d/proposal.md", 0, {
      kind: "file",
      title: "proposal.md",
      folder: "~\\Downloads",
    });
    expect(matchesFilter(file, "proposal")).toBe(true);
    expect(matchesFilter(file, "downloads")).toBe(true);
    expect(matchesFilter(file, "archive")).toBe(false);
  });

  const a = note("a.md", NOW, { title: "Server IPs and ports", snippet: "api 10.0.0.4 db" });
  const b = note("b.md", NOW, { title: "Weekly checklist", snippet: "backups, patches" });

  it("keeps everything when nothing is typed", () => {
    expect(filterNotes([a, b], "")).toEqual([a, b]);
    expect(filterNotes([a, b], "   ")).toEqual([a, b]);
  });

  it("matches words in the title or the snippet, ignoring case and accents", () => {
    expect(filterNotes([a, b], "server")).toEqual([a]);
    expect(filterNotes([a, b], "PATCHES")).toEqual([b]);
    expect(filterNotes([a, b], "10.0.0.4 api")).toEqual([a]);
    expect(filterNotes([a, b], "server patches")).toEqual([]);
    expect(
      matchesFilter(note("c.md", NOW, { title: "Reunião de revisão" }), "reuniao revisao"),
    ).toBe(true);
  });

  it("also keeps the notes the search found by their text", () => {
    expect(filterNotes([a, b], "backups", new Set(["a.md"]))).toEqual([a, b]);
    expect(filterNotes([a, b], "zzz", new Set(["b.md"]))).toEqual([b]);
    expect(filterNotes([a, b], "", new Set(["b.md"]))).toEqual([a, b]);
  });
});

describe("reordering the pinned notes", () => {
  const order = ["a.md", "b.md", "c.md"];

  it("moves one place with Alt+Up and Alt+Down, and not past the ends", () => {
    expect(movePinned(order, "b.md", -1)).toEqual(["b.md", "a.md", "c.md"]);
    expect(movePinned(order, "b.md", 1)).toEqual(["a.md", "c.md", "b.md"]);
    expect(movePinned(order, "a.md", -1)).toEqual(order);
    expect(movePinned(order, "c.md", 1)).toEqual(order);
    expect(movePinned(order, "zzz.md", 1)).toEqual(order);
  });

  it("takes the place of the note it is dropped on", () => {
    expect(dropPinned(order, "c.md", "a.md")).toEqual(["c.md", "a.md", "b.md"]);
    expect(dropPinned(order, "a.md", "c.md")).toEqual(["b.md", "c.md", "a.md"]);
    expect(dropPinned(order, "a.md", "a.md")).toEqual(order);
    expect(dropPinned(order, "a.md", "unpinned.md")).toEqual(order);
    expect(dropPinned(order, "unpinned.md", "a.md")).toEqual(order);
  });

  it("does not change the order it was given", () => {
    const copy = [...order];
    movePinned(order, "b.md", 1);
    dropPinned(order, "c.md", "a.md");
    expect(order).toEqual(copy);
  });
});

describe("jumping between notes", () => {
  const pinned = ["a", "b", "c"].map((p) => note(`${p}.md`, NOW, { pinned: true }));

  it("Ctrl+1 to Ctrl+9 go to the pinned notes in order", () => {
    expect(pinnedAt(pinned, 1)?.path).toBe("a.md");
    expect(pinnedAt(pinned, 3)?.path).toBe("c.md");
    expect(pinnedAt(pinned, 4)).toBeNull();
    expect(pinnedAt(pinned, 0)).toBeNull();
    expect(pinnedAt(pinned, 10)).toBeNull();
  });

  it("goes to the next and the previous note, around", () => {
    const shown = ["a.md", "b.md", "c.md"];
    expect(neighbour(shown, "a.md", 1)).toBe("b.md");
    expect(neighbour(shown, "c.md", 1)).toBe("a.md");
    expect(neighbour(shown, "a.md", -1)).toBe("c.md");
    expect(neighbour(shown, null, 1)).toBe("a.md");
    expect(neighbour(shown, null, -1)).toBe("c.md");
    expect(neighbour(shown, "gone.md", 1)).toBe("a.md");
    expect(neighbour([], "a.md", 1)).toBeNull();
  });
});

describe("shortFolder", () => {
  it("leaves a label that fits as it is", () => {
    expect(shortFolder("~\\Downloads")).toBe("~\\Downloads");
    expect(shortFolder("~/Documents/plans", 40)).toBe("~/Documents/plans");
  });

  it("cuts a long one in the middle, so the folders that tell two files apart stay", () => {
    const a = "~\\AppData\\Local\\Temp\\claude\\scratchpad\\loose\\a";
    const b = "~\\AppData\\Local\\Temp\\claude\\scratchpad\\loose\\b";
    expect(shortFolder(a)).toBe("~\\…\\Temp\\claude\\scratchpad\\loose\\a");
    expect(shortFolder(b)).toBe("~\\…\\Temp\\claude\\scratchpad\\loose\\b");
    expect(shortFolder(a)).not.toBe(shortFolder(b));
    expect(shortFolder(a).length).toBeLessThanOrEqual(34);
    expect(shortFolder(a, 20)).toBe("~\\…\\loose\\a");
    // the same on a system that separates with a slash
    expect(shortFolder("~/projects/clients/acme/docs/proposals/2026/q4", 30)).toBe(
      "~/…/docs/proposals/2026/q4",
    );
  });

  it("shows the end of a folder name that is long by itself", () => {
    const long = `~\\${"x".repeat(60)}\\sub`;
    const short = shortFolder(long, 24);
    expect(short.length).toBeLessThanOrEqual(24);
    expect(short.endsWith("sub")).toBe(true);
    expect(shortFolder(`C:${"y".repeat(60)}`, 20)).toHaveLength(20);
  });
});

describe("fullPlace", () => {
  it("is the folder label and the name, with the separator of the label", () => {
    expect(fullPlace("~\\Downloads", "proposal.md")).toBe("~\\Downloads\\proposal.md");
    expect(fullPlace("~/Documents", "todo.txt")).toBe("~/Documents/todo.txt");
    expect(fullPlace(null, "proposal.md")).toBe("proposal.md");
  });
});

describe("placeOf", () => {
  it("names the file and the folder it is in, whichever the separator", () => {
    expect(placeOf("C:\\work\\plan\\ask.md")).toEqual({ file: "ask.md", folder: "plan" });
    expect(placeOf("/home/ana/plan/ask.md")).toEqual({ file: "ask.md", folder: "plan" });
    expect(placeOf("C:\\ask.md")).toEqual({ file: "ask.md", folder: "C:" });
    expect(placeOf("ask.md")).toEqual({ file: "ask.md", folder: "" });
    expect(placeOf("")).toEqual({ file: "", folder: "" });
  });
});

describe("saveStatus", () => {
  const facts = {
    shown: true,
    readOnly: false,
    isFile: false,
    dirty: false,
    saving: false,
    savingSlow: false,
  };

  it("says Saved for a note that is on disk, and nothing while it is typed in or being saved", () => {
    expect(saveStatus(facts)).toBe("saved");
    expect(saveStatus({ ...facts, dirty: true })).toBe("none");
    expect(saveStatus({ ...facts, saving: true })).toBe("none");
    expect(saveStatus({ ...facts, saving: true, savingSlow: true })).toBe("saving");
  });

  it("says nothing in a window that can only read, where nothing is saved", () => {
    expect(saveStatus({ ...facts, readOnly: true })).toBe("none");
    expect(saveStatus({ ...facts, readOnly: true, dirty: true })).toBe("none");
    expect(saveStatus({ ...facts, readOnly: true, isFile: true })).toBe("none");
  });

  it("says nothing without an open note, and Unsaved changes for a loose file with typed text", () => {
    expect(saveStatus({ ...facts, shown: false })).toBe("none");
    expect(saveStatus({ ...facts, isFile: true })).toBe("note");
    expect(saveStatus({ ...facts, isFile: true, dirty: true })).toBe("unsaved");
  });
});
