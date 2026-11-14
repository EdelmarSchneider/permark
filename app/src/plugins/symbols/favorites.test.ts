// Favorite characters and pinned groups, one step at a time: order kept, exact code points, limits of the settings.
import { describe, expect, it } from "vitest";
import type { SymbolFavorites } from "../../api";
import {
  hasChar,
  hasGroup,
  MAX_FAVORITE_CHARS,
  MAX_FAVORITE_GROUPS,
  withChar,
  withGroup,
  withoutChar,
  withoutGroup,
} from "./favorites";

function favorites(chars: string[], groups: string[] = []): SymbolFavorites {
  return { v: 1, chars, groups };
}

describe("favorite characters", () => {
  it("adds in the order added, from nothing too", () => {
    const one = withChar(null, "α");
    expect(one).toEqual(favorites(["α"]));
    expect(withChar(one, "→")).toEqual(favorites(["α", "→"]));
  });

  it("a repeat add is the same value (nothing to write)", () => {
    const f = favorites(["α"], ["greek"]);
    expect(withChar(f, "α")).toBe(f);
  });

  it("compares exact code points, never normalized", () => {
    const heart = withChar(null, "❤");
    const both = withChar(heart, "❤️");
    expect(both?.chars).toEqual(["❤", "❤️"]);
    const ohm = withChar(null, "Ω");
    expect(withChar(ohm, "Ω")?.chars).toEqual(["Ω", "Ω"]);
    expect(hasChar(ohm, "Ω")).toBe(false);
    expect(withoutChar(ohm, "Ω")).toBe(ohm);
  });

  it("removes one; both lists empty -> null (the key goes away)", () => {
    expect(withoutChar(favorites(["α", "β"]), "α")).toEqual(favorites(["β"]));
    expect(withoutChar(favorites(["α"], ["greek"]), "α")).toEqual(favorites([], ["greek"]));
    expect(withoutChar(favorites(["α"]), "α")).toBeNull();
    expect(withoutChar(null, "α")).toBeNull();
  });

  it("refuses what the settings would refuse: too many, too long", () => {
    const full = favorites(
      Array.from({ length: MAX_FAVORITE_CHARS }, (_, n) => String.fromCodePoint(0x2200 + n)),
    );
    expect(withChar(full, "α")).toBe(full);
    const long = "→".repeat(17);
    expect(withChar(null, long)).toBeNull();
    expect(withChar(null, "→".repeat(16))?.chars).toEqual(["→".repeat(16)]);
    // an emoji ZWJ sequence counts by code points, not by UTF-16 units
    const family = "👨‍👩‍👧‍👦";
    expect(withChar(null, family)?.chars).toEqual([family]);
  });

  it("never changes the value it got", () => {
    const f = favorites(["α"]);
    withChar(f, "β");
    withoutChar(f, "α");
    expect(f).toEqual(favorites(["α"]));
  });
});

describe("pinned groups", () => {
  it("adds in the order added; a repeat is the same value", () => {
    const one = withGroup(null, "greek");
    expect(one).toEqual(favorites([], ["greek"]));
    const two = withGroup(one, "arrows");
    expect(two).toEqual(favorites([], ["greek", "arrows"]));
    expect(withGroup(two, "greek")).toBe(two);
    expect(hasGroup(two, "arrows")).toBe(true);
    expect(hasGroup(null, "arrows")).toBe(false);
  });

  it("removes one and keeps a stored name no group has", () => {
    const f = favorites(["α"], ["gone", "greek"]);
    expect(withoutGroup(f, "greek")).toEqual(favorites(["α"], ["gone"]));
    expect(withGroup(f, "arrows")?.groups).toEqual(["gone", "greek", "arrows"]);
    expect(withoutGroup(favorites([], ["greek"]), "greek")).toBeNull();
    expect(withoutGroup(f, "arrows")).toBe(f);
  });

  it("refuses more than the settings keep", () => {
    const full = favorites(
      [],
      Array.from({ length: MAX_FAVORITE_GROUPS }, (_, n) => `g${n}`),
    );
    expect(withGroup(full, "greek")).toBe(full);
  });
});
