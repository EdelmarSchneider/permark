// The picker's model: the groups and entries the settings payload ships, scanned and filtered by name.
import { describe, expect, it } from "vitest";
import { mockApi } from "../../../api/mock";
import type { CharacterEntry, CharacterGroup } from "../../../api";
import { GROUP_RENDER_CAP, QUERY_RENDER_CAP, groupsFor, scanGroups } from "./characters";

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
    expect(groupNames(groupsFor("symbols", groups))).toEqual(["greek", "arrows"]);
    const shown = scanGroups("symbols", groups, "");
    expect(groupNames(shown)).toEqual(["greek", "arrows"]);
    expect(charsOf(shown)).toEqual(["α", "π", "Σ", "→", "←", "⇔"]);
  });

  it("shows every entry on an empty query (a blank one behaves the same)", async () => {
    const groups = await payload();
    expect(total(scanGroups("emoji", groups, ""))).toBe(3);
    expect(total(scanGroups("emoji", groups, "   "))).toBe(3);
    expect(total(scanGroups("symbols", groups, ""))).toBe(6);
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
