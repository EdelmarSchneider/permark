// The arrangements saved under a name, pure: what a name is, when two names are one, what was saved made safe (the
// same reader as the window's own arrangement), the list's changes (save | replace, rename, delete, put back).
import { describe, expect, it } from "vitest";
import { defaultArrangement, FILES, OUTLINE, SEARCH, type Arrangement } from "./arrangement";
import {
  deleteLayout,
  findLayout,
  foldName,
  layoutName,
  MAX_LAYOUT_NAME,
  MAX_LAYOUTS,
  parseLayouts,
  renameLayout,
  restoreLayout,
  saveLayout,
  type Layout,
} from "./layouts";

/** An arrangement apart from the default: Search alone at the bottom. */
const bottomSearch: Arrangement = {
  v: 2,
  left: [{ views: [FILES], share: 1 }],
  right: [{ views: [OUTLINE], share: 1 }],
  bottom: [{ views: [SEARCH], share: 1 }],
};
const names = (items: readonly Layout[]): string[] => items.map((item) => item.name);
/** `count` layouts named `L1`..: the default arrangement each. */
const many = (count: number): Layout[] =>
  Array.from({ length: count }, (_, at) => ({
    name: `L${at + 1}`,
    arrangement: defaultArrangement(),
  }));
/** "é" as one character, and as "e" with a combining accent. */
const COMPOSED = "café";
const DECOMPOSED = "café";

describe("a layout's name", () => {
  it("is kept trimmed and composed", () => {
    expect(layoutName("  Review  ")).toBe("Review");
    expect(layoutName("\tWide desk\n")).toBe("Wide desk");
    expect(layoutName(DECOMPOSED)).toBe(COMPOSED);
    expect(layoutName(COMPOSED)).toBe(COMPOSED);
    expect(layoutName("two  words")).toBe("two  words");
  });

  it("has 1 to 40 characters, counted as a person reads them (an emoji is one)", () => {
    expect(MAX_LAYOUT_NAME).toBe(40);
    expect(layoutName("")).toBeNull();
    expect(layoutName("   ")).toBeNull();
    expect(layoutName("a")).toBe("a");
    expect(layoutName("a".repeat(40))).toBe("a".repeat(40));
    expect(layoutName("a".repeat(41))).toBeNull();
    // 40 code points, 41 UTF-16 units
    const withEmoji = `${"a".repeat(39)}\u{1F600}`;
    expect(withEmoji).toHaveLength(41);
    expect(layoutName(withEmoji)).toBe(withEmoji);
    expect(layoutName(`${withEmoji}b`)).toBeNull();
    expect(layoutName("\u{1F600}".repeat(40))).toBe("\u{1F600}".repeat(40));
    expect(layoutName("\u{1F600}".repeat(41))).toBeNull();
    // the count is of the composed name
    expect(layoutName(`${"a".repeat(39)}é`)).toBe(`${"a".repeat(39)}é`);
  });

  it("holds no control character, and no half of a character pair", () => {
    for (const bad of ["a\tb", "a\nb", "a\u0000b", "a\u007fb", "a\u0085b", "a\ud83db"]) {
      expect(layoutName(bad), JSON.stringify(bad)).toBeNull();
    }
  });
});

describe("two names are one layout", () => {
  it("when they differ by case only", () => {
    expect(foldName("Review")).toBe(foldName("review"));
    expect(foldName("REVIEW")).toBe(foldName("rEvIeW"));
    expect(foldName("Review")).not.toBe(foldName("Reviews"));
  });

  it("when they differ by how an accent is written", () => {
    expect(foldName(COMPOSED)).toBe(foldName(DECOMPOSED));
    expect(foldName(COMPOSED.toUpperCase())).toBe(foldName(DECOMPOSED));
  });

  it("a slash is a character like any other", () => {
    expect(foldName("a\\b")).not.toBe(foldName("a/b"));
  });

  it("finds a layout by any spelling of its name", () => {
    const items = saveLayout([], COMPOSED, bottomSearch);
    expect(findLayout(items, DECOMPOSED.toUpperCase())?.name).toBe(COMPOSED);
    expect(findLayout(items, "tea")).toBeUndefined();
  });
});

describe("saving a layout", () => {
  it("adds it at the end, its name as kept", () => {
    const one = saveLayout([], "  Review ", bottomSearch);
    expect(one).toEqual([{ name: "Review", arrangement: bottomSearch }]);
    const two = saveLayout(one, "Writing", defaultArrangement());
    expect(names(two)).toEqual(["Review", "Writing"]);
    expect(two[0]).toBe(one[0]);
  });

  it("under a name that folds like one kept replaces that one in place, with the new spelling", () => {
    const items = saveLayout(saveLayout(many(2), "Review", bottomSearch), "Last", bottomSearch);
    const next = saveLayout(items, "REVIEW", defaultArrangement());
    expect(names(next)).toEqual(["L1", "L2", "REVIEW", "Last"]);
    expect(next[2]?.arrangement).toEqual(defaultArrangement());
    expect(next[3]).toBe(items[3]);
    expect(names(saveLayout(next, DECOMPOSED, bottomSearch))).toEqual([
      "L1",
      "L2",
      "REVIEW",
      "Last",
      COMPOSED,
    ]);
  });

  it("a list of 20 takes no new one; one of its own can still be replaced", () => {
    expect(MAX_LAYOUTS).toBe(20);
    const full = many(20);
    expect(saveLayout(full, "One more", bottomSearch)).toBe(full);
    const next = saveLayout(full, "l7", bottomSearch);
    expect(next).toHaveLength(20);
    expect(next[6]).toEqual({ name: "l7", arrangement: bottomSearch });
    expect(names(saveLayout(many(19), "One more", bottomSearch)).at(-1)).toBe("One more");
  });

  it("without a usable name, or as it already is: the same list", () => {
    const items = saveLayout([], "Review", bottomSearch);
    expect(saveLayout(items, "  ", defaultArrangement())).toBe(items);
    expect(saveLayout(items, "a".repeat(41), defaultArrangement())).toBe(items);
    expect(saveLayout(items, "Review", bottomSearch)).toBe(items);
  });
});

describe("renaming a layout", () => {
  const items = saveLayout(saveLayout([], "Review", bottomSearch), "Writing", defaultArrangement());

  it("keeps its place and its arrangement", () => {
    const next = renameLayout(items, "review", " Proofing ");
    expect(names(next)).toEqual(["Proofing", "Writing"]);
    expect(next[0]?.arrangement).toBe(bottomSearch);
    expect(next[1]).toBe(items[1]);
  });

  it("to a name another layout has is refused, however it is spelled", () => {
    expect(renameLayout(items, "Review", "Writing")).toBe(items);
    expect(renameLayout(items, "Review", "  WRITING ")).toBe(items);
  });

  it("to another spelling of its own name is fine", () => {
    expect(names(renameLayout(items, "Review", "REVIEW"))).toEqual(["REVIEW", "Writing"]);
    const accented = saveLayout([], DECOMPOSED, bottomSearch);
    expect(names(renameLayout(accented, COMPOSED, "Café"))).toEqual(["Café"]);
  });

  it("to no usable name, to the name it has, or of a layout that is not there: the same list", () => {
    expect(renameLayout(items, "Review", " ")).toBe(items);
    expect(renameLayout(items, "Review", "a".repeat(41))).toBe(items);
    expect(renameLayout(items, "Review", "Review")).toBe(items);
    expect(renameLayout(items, "Gone", "Other")).toBe(items);
  });
});

describe("deleting a layout, and putting it back", () => {
  const items = many(3);

  it("takes it out by any spelling of its name; none of that name: the same list", () => {
    expect(names(deleteLayout(items, "l2"))).toEqual(["L1", "L3"]);
    expect(deleteLayout(items, "Gone")).toBe(items);
  });

  it("puts it back at the place it had", () => {
    const [first, second, third] = items;
    if (!first || !second || !third) throw new Error("no layouts");
    expect(names(restoreLayout(deleteLayout(items, "L1"), first, 0))).toEqual(["L1", "L2", "L3"]);
    expect(names(restoreLayout(deleteLayout(items, "L2"), second, 1))).toEqual(["L1", "L2", "L3"]);
    expect(names(restoreLayout(deleteLayout(items, "L3"), third, 2))).toEqual(["L1", "L2", "L3"]);
    // a list that got shorter meanwhile: at its end
    expect(names(restoreLayout([first], third, 2))).toEqual(["L1", "L3"]);
  });

  it("puts nothing back over a layout of that name, or into a full list", () => {
    const [first] = items;
    if (!first) throw new Error("no layouts");
    expect(restoreLayout(items, { ...first, name: "l1" }, 0)).toBe(items);
    const full = many(20);
    expect(restoreLayout(full, { name: "Extra", arrangement: bottomSearch }, 3)).toBe(full);
  });
});

describe("the saved layouts, read", () => {
  const panels = { v: 2, left: [{ views: [FILES], share: 1 }], right: [], bottom: [] };
  const item = (name: unknown, arrangement: unknown = panels) => ({ name, arrangement });

  it("absent, damaged or of another version: no layout", () => {
    for (const raw of [
      null,
      undefined,
      "layouts",
      7,
      [],
      {},
      { v: 1 },
      { v: 1, items: "none" },
      { v: 1, items: {} },
      { items: [item("Review")] },
      { v: 2, items: [item("Review")] },
      { v: "1", items: [item("Review")] },
    ]) {
      expect(parseLayouts(raw), JSON.stringify(raw)).toEqual([]);
    }
  });

  it("keeps each readable layout in order, its name as kept, its arrangement made safe", () => {
    const read = parseLayouts({
      v: 1,
      items: [
        item("  Review "),
        item(DECOMPOSED, {
          v: 2,
          left: [{ views: [FILES, FILES, SEARCH], share: 3 }],
          right: [{ views: [SEARCH, OUTLINE], share: 0.5 }, { views: [] }],
          bottom: [],
        }),
      ],
    });
    expect(names(read)).toEqual(["Review", COMPOSED]);
    expect(read[0]?.arrangement).toEqual(panels);
    // a view listed twice keeps its first place, an empty group goes, shares sum 1
    expect(read[1]?.arrangement).toEqual({
      v: 2,
      left: [{ views: [FILES, SEARCH], share: 1 }],
      right: [{ views: [OUTLINE], share: 1 }],
      bottom: [],
    });
  });

  it("drops an item without a usable name or with an arrangement that cannot be read", () => {
    const read = parseLayouts({
      v: 1,
      items: [
        item(""),
        item(7),
        item("a".repeat(41)),
        item("tab\there"),
        "Review",
        null,
        { name: "No arrangement" },
        item("Not a table", "wide"),
        item("Old column", { v: 1, groups: [{ views: [OUTLINE], share: 1 }] }),
        item("Newer", { ...panels, v: 3 }),
        item("No bottom", { v: 2, left: [], right: [] }),
        item("Kept"),
      ],
    });
    expect(names(read)).toEqual(["Kept"]);
  });

  it("drops a second item whose name folds like one before it", () => {
    const read = parseLayouts({
      v: 1,
      items: [item("Review"), item("Writing"), item("REVIEW", { ...panels, right: [] })],
    });
    expect(names(read)).toEqual(["Review", "Writing"]);
    expect(read[0]?.arrangement).toEqual(panels);
  });

  it("keeps the first 20 of a longer list", () => {
    const items = Array.from({ length: 25 }, (_, at) => item(`L${at + 1}`));
    const read = parseLayouts({ v: 1, items });
    expect(read).toHaveLength(20);
    expect(names(read).at(-1)).toBe("L20");
    // an unusable item takes no place
    expect(names(parseLayouts({ v: 1, items: [item(""), ...items] })).at(-1)).toBe("L20");
  });

  it("keeps the ids of views nobody offers now (a plugin that is off finds its place again)", () => {
    const read = parseLayouts({
      v: 1,
      items: [
        item("With plugins", {
          v: 2,
          left: [{ views: [FILES, "gone.panel"], share: 1 }],
          right: [],
          bottom: [{ views: ["other.gone"], share: 1 }],
        }),
      ],
    });
    expect(read[0]?.arrangement.left[0]?.views).toEqual([FILES, "gone.panel"]);
    expect(read[0]?.arrangement.bottom[0]?.views).toEqual(["other.gone"]);
  });
});
