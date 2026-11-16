// What the symbols panel draws: favorites, pinned groups, the other groups, collapsed | expanded, searched.
import { describe, expect, it } from "vitest";
import type { CharacterEntry, CharacterGroup, SymbolFavorites } from "../../api";
import { QUERY_RENDER_CAP } from "../../core/editing/visual/characters";
import { canInsert, DEFAULT_COLLAPSED, sectionsOf, type Section } from "./model";

function entry(char: string, ...names: string[]): CharacterEntry {
  return { char, names };
}

const GREEK: CharacterGroup = {
  name: "greek",
  entries: [entry("α", "alpha"), entry("β", "beta"), entry("Ω", "omega")],
};
const ARROWS: CharacterGroup = {
  name: "arrows",
  entries: [entry("→", "rightwards arrow"), entry("←", "leftwards arrow")],
};
const EMOJI: CharacterGroup = {
  name: "emoji",
  entries: [entry("❤️", "red heart"), entry("😀", "grinning face")],
};
// a long group: the panel shows all of it, no cap
const MANY: CharacterGroup = {
  name: "many",
  entries: Array.from({ length: 150 }, (_, n) =>
    entry(String.fromCodePoint(0x2200 + n), `math ${n}`),
  ),
};
const GROUPS: CharacterGroup[] = [GREEK, EMOJI, ARROWS, MANY];

function favorites(chars: string[], groups: string[] = []): SymbolFavorites {
  return { v: 1, chars, groups };
}

/** One line per section: what a test reads at a glance. */
function outline(sections: readonly Section[]): string[] {
  return sections.map((section) => {
    switch (section.kind) {
      case "favorites":
        return `favorites[${section.entries.map((e) => e.char).join("")}]`;
      case "divider":
        return "---";
      case "group":
        return `${section.pinned ? "*" : ""}${section.name}${section.expanded ? "+" : "-"}${section.entries.length}/${section.total}`;
      default:
        return section satisfies never;
    }
  });
}

describe("the panel sections", () => {
  it("lists favorites, then the symbol groups in payload order, then the emoji group collapsed", () => {
    const sections = sectionsOf(GROUPS, null, "", DEFAULT_COLLAPSED);
    expect(outline(sections)).toEqual([
      "favorites[]",
      "greek+3/3",
      "arrows+2/2",
      "many+150/150",
      "emoji-0/2",
    ]);
  });

  it("a collapsed group shows its count and no entries; an expanded one shows every entry", () => {
    const sections = sectionsOf(GROUPS, null, "", new Set(["greek"]));
    expect(outline(sections)).toEqual([
      "favorites[]",
      "greek-0/3",
      "arrows+2/2",
      "many+150/150",
      "emoji+2/2",
    ]);
  });

  it("a favorite char takes the entry of the groups; one absent from them draws with no name", () => {
    const sections = sectionsOf(GROUPS, favorites(["→", "★"]), "", DEFAULT_COLLAPSED);
    const fav = sections[0];
    expect(fav?.kind).toBe("favorites");
    if (fav?.kind !== "favorites") return;
    expect(fav.entries).toEqual([entry("→", "rightwards arrow"), entry("★")]);
  });

  it("keeps the exact code points: a heart without the emoji selector is another character", () => {
    const sections = sectionsOf(GROUPS, favorites(["❤"]), "", DEFAULT_COLLAPSED);
    const fav = sections[0];
    if (fav?.kind !== "favorites") throw new Error("no favorites section");
    expect(fav.entries).toEqual([entry("❤")]);
  });

  it("pinned groups come after the favorites in file order, then a divider; they are not repeated below", () => {
    const sections = sectionsOf(
      GROUPS,
      favorites([], ["arrows", "emoji", "greek", "gone"]),
      "",
      DEFAULT_COLLAPSED,
    );
    expect(outline(sections)).toEqual([
      "favorites[]",
      "*greek+3/3",
      "*emoji-0/2",
      "*arrows+2/2",
      "---",
      "many+150/150",
    ]);
  });

  it("no divider when every group is pinned", () => {
    const sections = sectionsOf([GREEK], favorites([], ["greek"]), "", DEFAULT_COLLAPSED);
    expect(outline(sections)).toEqual(["favorites[]", "*greek+3/3"]);
  });

  it("a query expands every matching group, collapsed or not, and filters the favorites with the same matcher", () => {
    const all = new Set(GROUPS.map((g) => g.name));
    const sections = sectionsOf(GROUPS, favorites(["β", "→"]), "arrow", all);
    expect(outline(sections)).toEqual(["favorites[→]", "arrows+2/2"]);
  });

  it("a query hides the favorites when none answers it", () => {
    const sections = sectionsOf(GROUPS, favorites(["β"]), "heart", DEFAULT_COLLAPSED);
    expect(outline(sections)).toEqual(["emoji+1/1"]);
  });

  it("finds by the pasted character and by its code point, emoji too", () => {
    expect(outline(sectionsOf(GROUPS, null, "U+2192", DEFAULT_COLLAPSED))).toEqual(["arrows+1/1"]);
    expect(outline(sectionsOf(GROUPS, null, "2192", DEFAULT_COLLAPSED))).toEqual(["arrows+1/1"]);
    expect(outline(sectionsOf(GROUPS, null, "😀", DEFAULT_COLLAPSED))).toEqual(["emoji+1/1"]);
    // a favorite absent from the groups answers its code point
    expect(outline(sectionsOf(GROUPS, favorites(["★"]), "U+2605", DEFAULT_COLLAPSED))).toEqual([
      "favorites[★]",
    ]);
  });

  it("a query keeps the picker's cap over the groups", () => {
    const sections = sectionsOf(GROUPS, null, "math", DEFAULT_COLLAPSED);
    const shown = sections.reduce((n, s) => n + (s.kind === "group" ? s.entries.length : 0), 0);
    expect(shown).toBe(Math.min(150, QUERY_RENDER_CAP));
  });

  it("a blank query is no query", () => {
    expect(outline(sectionsOf(GROUPS, null, "  ", DEFAULT_COLLAPSED))).toEqual(
      outline(sectionsOf(GROUPS, null, "", DEFAULT_COLLAPSED)),
    );
  });

  it("gives every section a distinct key, also for groups of the same name", () => {
    const twice: CharacterGroup[] = [GREEK, { ...GREEK, entries: [entry("γ", "gamma")] }];
    const keys = sectionsOf(twice, favorites([], ["greek"]), "", new Set()).map((s) => s.key);
    expect(new Set(keys).size).toBe(keys.length);
  });
});

describe("whether the panel can insert", () => {
  it("needs a document in Edit or Source", () => {
    expect(canInsert("edit", true)).toBe("ok");
    expect(canInsert("source", true)).toBe("ok");
    expect(canInsert("view", true)).toBe("view");
    expect(canInsert("view", false)).toBe("none");
    expect(canInsert("edit", false)).toBe("none");
  });
});
