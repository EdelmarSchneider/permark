// The picker's model: the groups and entries the settings payload ships, scanned and filtered by name.
import { describe, expect, it } from "vitest";
import { mockApi } from "../../../api/mock";
import type { CharacterEntry, CharacterGroup } from "../../../api";
import {
  GROUP_RENDER_CAP,
  QUERY_RENDER_CAP,
  cellLabel,
  cellName,
  codePoints,
  entryName,
  groupsFor,
  groupLabel,
  isBlankQuery,
  isInvisible,
  matchEntries,
  scanGroups,
} from "./characters";

/** The picker payload of the mock profile (the groups the backend ships as built-ins). */
async function payload(): Promise<CharacterGroup[]> {
  const result = await mockApi.settingsGet();
  if (!result.ok) throw new Error("the mock settings are unreadable");
  return result.value.characters;
}

function entry(char: string, ...names: string[]): CharacterEntry {
  return { char, names };
}

function group(name: string, entries: CharacterEntry[]): CharacterGroup {
  return { name, entries };
}

const groupNames = (shown: readonly CharacterGroup[]): string[] => shown.map((g) => g.name);

const charsOf = (shown: readonly { entries: CharacterEntry[] }[]): string[] =>
  shown.flatMap((g) => g.entries.map((e) => e.char));

const total = (shown: readonly { entries: unknown[] }[]): number =>
  shown.reduce((n, g) => n + g.entries.length, 0);

describe("the picker groups (the settings payload is the source)", () => {
  it("scans the payload's groups in order, emoji apart from the symbols", async () => {
    const groups = await payload();
    expect(groupNames(groupsFor("emoji", groups))).toEqual(["emoji"]);
    expect(groupNames(groupsFor("symbols", groups))).toEqual(["greek", "arrows", "spaces"]);
    const shown = scanGroups("symbols", groups, "");
    expect(groupNames(shown)).toEqual(["greek", "arrows", "spaces"]);
    expect(charsOf(shown)).toEqual(["α", "π", "Σ", "→", "←", "⇔", "\u00A0", "\u2009", "\u2060"]);
  });

  it("shows every entry on an empty query (a blank one behaves the same)", async () => {
    const groups = await payload();
    expect(total(scanGroups("emoji", groups, ""))).toBe(3);
    expect(total(scanGroups("emoji", groups, "   "))).toBe(3);
    expect(total(scanGroups("symbols", groups, ""))).toBe(9);
  });

  it("filters by name: a word inside a name, a prefix, a subsequence", async () => {
    const groups = await payload();
    const arrow = scanGroups("symbols", groups, "arrow");
    expect(groupNames(arrow)).toEqual(["arrows"]);
    expect(charsOf(arrow)).toEqual(["→", "←", "⇔"]);
    expect(charsOf(scanGroups("symbols", groups, "sig"))).toEqual(["Σ"]);
    expect(charsOf(scanGroups("symbols", groups, "sum"))).toEqual(["Σ"]);
    expect(charsOf(scanGroups("symbols", groups, "left"))).toEqual(["←", "⇔"]);
  });

  it("ranks the best-named match first when several entries answer", async () => {
    const groups = await payload();
    expect(charsOf(scanGroups("symbols", groups, "left arrow"))).toEqual(["←", "⇔"]);
    expect(charsOf(scanGroups("emoji", groups, "check"))).toEqual(["✅"]);
  });

  it("searches accented names with the accents folded, and emoji by their names", async () => {
    const units = [group("units", [entry("Å", "Ångström"), entry("µ", "micro sign", "mu")])];
    expect(charsOf(scanGroups("symbols", units, "angström"))).toEqual(["Å"]);
    expect(charsOf(scanGroups("symbols", units, "angstrom"))).toEqual(["Å"]);
    expect(charsOf(scanGroups("symbols", units, "ÄNGSTROM"))).toEqual(["Å"]);
    expect(charsOf(scanGroups("symbols", units, "micro"))).toEqual(["µ"]);
    const groups = await payload();
    expect(charsOf(scanGroups("emoji", groups, "grinning"))).toEqual(["😀"]);
    expect(charsOf(scanGroups("emoji", groups, "done"))).toEqual(["✅"]);
    expect(charsOf(scanGroups("emoji", groups, "thumbs"))).toEqual(["👍"]);
    expect(scanGroups("symbols", groups, "grinning")).toEqual([]);
  });

  it("drops the groups with no entry left", async () => {
    const groups = await payload();
    expect(scanGroups("symbols", groups, "zebra")).toEqual([]);
    expect(scanGroups("emoji", groups, "zzqxvw")).toEqual([]);
  });

  it("caps what a group renders with no query, and caps a query's answers", () => {
    const wide = Array.from({ length: 200 }, (_, i) =>
      entry(String.fromCharCode(0x2100 + i), `entry ${i}`),
    );
    const shown = scanGroups("symbols", [group("wide", wide)], "");
    expect(shown[0]?.entries.length).toBe(GROUP_RENDER_CAP);
    expect(shown[0]?.entries.map((e) => e.char)).toEqual(
      wide.slice(0, GROUP_RENDER_CAP).map((e) => e.char),
    );
    const groups = Array.from({ length: 4 }, (_, g) =>
      group(
        `g${g}`,
        Array.from({ length: 90 }, (_, i) =>
          entry(String.fromCharCode(0x3000 + g * 100 + i), `mark ${i} here`),
        ),
      ),
    );
    const hits = scanGroups("symbols", groups, "mark");
    expect(total(hits)).toBe(QUERY_RENDER_CAP);
    expect(groupNames(hits)).toEqual(["g0", "g1", "g2"]);
  });
});

describe("search by the character itself or its code point", () => {
  const arrows = [
    group("arrows", [
      entry("⇒", "rightwards double arrow", "not 2192"),
      entry("→", "right arrow"),
      entry("→\uFE0E", "right arrow text style"),
      entry("←", "left arrow"),
    ]),
    group("spaces", [entry("\u00A0", "no-break space", "nbsp")]),
  ];

  it("a pasted character finds its entry, ranked above the entries that match by name", () => {
    expect(charsOf(scanGroups("symbols", arrows, "→"))[0]).toBe("→");
    // a pasted invisible character is a query, not a blank one
    expect(charsOf(scanGroups("symbols", arrows, "\u00A0"))).toEqual(["\u00A0"]);
  });

  it("U+2192, u+2192 and 2192 find the entries that are or start with that code point, first", () => {
    for (const query of ["U+2192", "u+2192", "2192", " U+2192 "]) {
      expect(charsOf(scanGroups("symbols", arrows, query)).slice(0, 2), query).toEqual([
        "→",
        "→\uFE0E",
      ]);
    }
    // a name that holds the digits comes after, though the groups file lists it first
    expect(charsOf(scanGroups("symbols", arrows, "2192"))).toEqual(["→", "→\uFE0E", "⇒"]);
    expect(charsOf(scanGroups("symbols", arrows, "u+00a0"))).toEqual(["\u00A0"]);
    // fewer than 4 or more than 6 hex digits read as a name, not a code point
    expect(charsOf(scanGroups("symbols", arrows, "21"))).not.toContain("→");
    expect(charsOf(scanGroups("symbols", arrows, "U+0021920"))).not.toContain("→");
  });

  it("a blank query of ASCII spaces still shows every group", () => {
    expect(groupNames(scanGroups("symbols", arrows, "  "))).toEqual(["arrows", "spaces"]);
  });
});

describe("what a cell shows and says", () => {
  it("tells the invisible characters (spaces, format characters) from the visible ones", () => {
    for (const char of ["\u00A0", "\u2009", "\u202F", "\u2007", "\u2060", "\u200B\u00A0"]) {
      expect(isInvisible(char), char.codePointAt(0)?.toString(16)).toBe(true);
    }
    for (const char of ["→", "a", "😀", "\u00A0a", ""]) expect(isInvisible(char)).toBe(false);
  });

  it("draws an invisible entry as its shortest name, uppercased and cut to five letters", () => {
    expect(cellLabel(entry("\u00A0", "no-break space", "nbsp"))).toBe("NBSP");
    expect(cellLabel(entry("\u2060", "word joiner", "wj"))).toBe("WJ");
    expect(cellLabel(entry("\u2009", "thin space"))).toBe("THIN");
    expect(cellLabel(entry("\u2007"))).toBe("2007");
    expect(cellLabel(entry("→", "right arrow"))).toBe("→");
  });

  it("names an entry by its first name, else by its code points", () => {
    expect(entryName(entry("→", "right arrow", "yields"))).toBe("right arrow");
    expect(entryName(entry("→"))).toBe("U+2192");
    expect(entryName(entry("👍🏽"))).toBe("U+1F44D U+1F3FD");
  });

  it("writes the code points of a character, the first three of a longer sequence", () => {
    expect(codePoints("α")).toBe("U+03B1");
    expect(codePoints("😀")).toBe("U+1F600");
    expect(codePoints("👍🏽")).toBe("U+1F44D U+1F3FD");
    expect(codePoints("👨‍👩‍👧")).toBe("U+1F468 U+200D U+1F469 …");
  });
});

describe("the label of a group", () => {
  it("translates the built-in names (any case) and shows any other name as the file gives it", () => {
    expect(groupLabel("spaces")).toBe("Spaces");
    expect(groupLabel(" Greek ")).toBe("Greek");
    expect(groupLabel("My marks")).toBe("My marks");
    expect(groupLabel("constructor")).toBe("constructor");
  });
});

describe("matching a list of entries", () => {
  it("ranks like a picker group, keeps the list on a blank query and has no cap", () => {
    const list = [entry("β", "beta"), entry("→", "rightwards arrow"), entry("α", "alpha")];
    expect(matchEntries("alpha", list).map((e) => e.char)).toEqual(["α"]);
    expect(matchEntries("U+2192", list).map((e) => e.char)).toEqual(["→"]);
    expect(matchEntries(" \t", list)).toEqual(list);
    const many = Array.from({ length: QUERY_RENDER_CAP + 5 }, (_, n) => entry(`${n}`, "same"));
    expect(matchEntries("same", many)).toHaveLength(QUERY_RENDER_CAP + 5);
  });

  it("a pasted no-break space is a query, ASCII blanks are none", () => {
    expect(isBlankQuery(" \n")).toBe(true);
    expect(isBlankQuery("\u00a0")).toBe(false);
  });
});

describe("the name of a cell", () => {
  it("is the entry's name; an invisible one whose label the name lacks says the label first", () => {
    expect(cellName(entry("α", "alpha"))).toBe("alpha");
    expect(cellName(entry("\u2009", "thin space"))).toBe("thin space");
    expect(cellName(entry("\u202F", "narrow no-break space", "nnbsp"))).toBe(
      "NNBSP, narrow no-break space",
    );
  });
});
