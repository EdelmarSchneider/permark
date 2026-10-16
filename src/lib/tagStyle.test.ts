import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { MESSAGES } from "./i18n";
import {
  COLOR_LABELS,
  gridMove,
  hasLook,
  ICON_LABELS,
  isTagColor,
  isTagIcon,
  readStyle,
  readStyles,
  TAG_COLORS,
  TAG_ICONS,
  withStyle,
} from "./tagStyle";
import { TONES } from "./tone";

describe("the palette and the icon set", () => {
  it("offer eight colors and twelve icons, each once", () => {
    expect(new Set(TAG_COLORS).size).toBe(8);
    expect(new Set(TAG_ICONS).size).toBe(12);
  });

  it("are the same lists the backend keeps", () => {
    const rust = readFileSync("src-tauri/src/meta/style.rs", "utf8");
    const list = (name: string): string[] => {
      const body = new RegExp(`pub const ${name}: \\[&str; \\d+\\] = \\[([^\\]]*)\\]`).exec(
        rust,
      )?.[1];
      return [...(body ?? "").matchAll(/"([^"]+)"/g)].map((m) => m[1]);
    };
    expect(list("COLORS")).toEqual([...TAG_COLORS]);
    expect(list("ICONS")).toEqual([...TAG_ICONS]);
  });

  it("have a name in every language, for the pickers and their tooltips", () => {
    for (const [locale, table] of Object.entries(MESSAGES)) {
      const t = table as Record<string, string>;
      for (const key of [...Object.values(COLOR_LABELS), ...Object.values(ICON_LABELS)]) {
        expect(t[key], `${locale} ${key}`).toBeTruthy();
      }
    }
  });

  it("only use icons the app's icon set has", () => {
    const registry = readFileSync("src/lib/components/Icon.svelte", "utf8");
    for (const icon of TAG_ICONS) {
      expect(registry, icon).toMatch(new RegExp(`^\\s+"?${icon}"?: \\w+,$`, "m"));
    }
  });

  it("know their own names", () => {
    expect(isTagColor("teal")).toBe(true);
    expect(isTagColor("Teal")).toBe(false);
    expect(isTagColor("chartreuse")).toBe(false);
    expect(isTagColor(null)).toBe(false);
    expect(isTagIcon("book-open")).toBe(true);
    expect(isTagIcon("rocket")).toBe(false);
    expect(isTagIcon("blue")).toBe(false);
  });
});

describe("reading the backend's answer", () => {
  it("keeps what this build can draw and leaves out the rest", () => {
    const styles = readStyles({
      spec: { color: "blue", icon: "book-open" },
      idea: { color: null, icon: "lightbulb" },
      later: { color: "chartreuse", icon: "rocket" },
      half: { color: "chartreuse", icon: "star" },
      broken: 5,
      empty: { color: null, icon: null },
    });
    expect([...styles.keys()].sort()).toEqual(["half", "idea", "spec"]);
    expect(styles.get("half")).toEqual({ color: null, icon: "star" });
    expect(styles.get("spec")).toEqual({ color: "blue", icon: "book-open" });
  });

  it("treats anything that is not a table of looks as no looks", () => {
    for (const bad of [null, undefined, 3, "x"]) expect(readStyles(bad).size).toBe(0);
    expect(readStyle(null)).toBeNull();
    expect(readStyle([])).toBeNull();
  });

  it("reads a tag called like an object's own members as an ordinary tag", () => {
    const styles = readStyles(
      JSON.parse(
        '{"__proto__":{"color":"red","icon":null},"constructor":{"color":"teal","icon":null}}',
      ),
    );
    expect(styles.get("__proto__")?.color).toBe("red");
    expect(styles.get("constructor")?.color).toBe("teal");
    expect(styles.get("toString")).toBeUndefined();
  });
});

describe("changing the looks", () => {
  it("replaces one tag's look and leaves the others", () => {
    const base = readStyles({ spec: { color: "blue", icon: null } });
    const next = withStyle(base, "idea", { color: null, icon: "lightbulb" });
    expect(next.get("idea")).toEqual({ color: null, icon: "lightbulb" });
    expect(next.get("spec")).toEqual({ color: "blue", icon: null });
    expect(base.has("idea")).toBe(false); // the old map is not touched
  });

  it("removes a tag's look when it has neither a color nor an icon", () => {
    const base = readStyles({ spec: { color: "blue", icon: null } });
    expect(withStyle(base, "spec", { color: null, icon: null }).has("spec")).toBe(false);
    expect(withStyle(base, "spec", null).has("spec")).toBe(false);
  });

  it("says whether a look changes anything", () => {
    expect(hasLook({ color: "red", icon: null })).toBe(true);
    expect(hasLook({ color: null, icon: "flag" })).toBe(true);
    expect(hasLook({ color: null, icon: null })).toBe(false);
    expect(hasLook(null)).toBe(false);
    expect(hasLook(undefined)).toBe(false);
  });
});

describe("moving through a group of choices with the keys", () => {
  // nine colors in one row
  it("steps left and right, around the ends, in a single row", () => {
    expect(gridMove("ArrowRight", 3, 9, 9)).toBe(4);
    expect(gridMove("ArrowRight", 8, 9, 9)).toBe(0);
    expect(gridMove("ArrowLeft", 0, 9, 9)).toBe(8);
    expect(gridMove("ArrowDown", 3, 9, 9)).toBe(4);
    expect(gridMove("ArrowUp", 3, 9, 9)).toBe(2);
  });

  // thirteen icons, seven to a row
  it("moves a row at a time in a grid, and stops at the edges", () => {
    expect(gridMove("ArrowDown", 2, 13, 7)).toBe(9);
    expect(gridMove("ArrowDown", 9, 13, 7)).toBe(12);
    expect(gridMove("ArrowUp", 9, 13, 7)).toBe(2);
    expect(gridMove("ArrowUp", 2, 13, 7)).toBe(0);
  });

  it("jumps to the ends with Home and End and ignores other keys", () => {
    expect(gridMove("Home", 5, 13, 7)).toBe(0);
    expect(gridMove("End", 5, 13, 7)).toBe(12);
    expect(gridMove("a", 5, 13, 7)).toBeNull();
    expect(gridMove("ArrowRight", 0, 0, 7)).toBeNull();
  });
});

// ---- the colors as design tokens ----------------------------------------------------------------------------------

const tokens = readFileSync("src/styles/tokens.css", "utf8");
const tagsCss = readFileSync("src/styles/tags.css", "utf8");

/** The `--name: value` pairs of the block that starts with `selector`. */
function block(selector: string): Record<string, string> {
  const start = tokens.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`no block for ${selector}`);
  const body = tokens.slice(tokens.indexOf("{", start) + 1, tokens.indexOf("}", start));
  const found: Record<string, string> = {};
  for (const m of body.matchAll(/--([\w-]+):\s*([^;]+);/g)) found[m[1]] = m[2].trim();
  return found;
}

const light = block(':root,\n[data-theme="light"]');
const dark = block('[data-theme="dark"]');
/** A tone of the light theme: white is the light theme itself, the others repaint some of its tokens. */
const soft = (name: string): Record<string, string> =>
  name === "white"
    ? light
    : { ...light, ...block(`:root[data-tone="${name}"]:not([data-theme="dark"])`) };

function rgb(hex: string): [number, number, number] {
  const h = hex.replace("#", "");
  return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16)) as [number, number, number];
}
function luminance([r, g, b]: [number, number, number]): number {
  const lin = (c: number): number => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}
function ratio(a: [number, number, number], b: [number, number, number]): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}
/** `color-mix(in srgb, fg pct%, bg)`, as the chip's background is drawn. */
function mixed(fg: string, percent: number, bg: string): [number, number, number] {
  const [f, b] = [rgb(fg), rgb(bg)];
  return f.map((c, i) => Math.round((c * percent) / 100 + b[i] * (1 - percent / 100))) as [
    number,
    number,
    number,
  ];
}

const tint = parseFloat(light["tag-tint"]);

describe("the tag colors as design tokens", () => {
  it("are plain colors in the light theme and in the dark one, and the tint is the same in both", () => {
    for (const set of [light, dark]) {
      for (const color of TAG_COLORS) {
        expect(set[`tag-${color}`], color).toMatch(/^#[0-9a-f]{6}$/i);
      }
    }
    expect(tint).toBeGreaterThan(0);
    expect(tint).toBeLessThan(30);
    expect(dark["tag-tint"]).toBeUndefined();
  });

  it("are all different from each other in each theme", () => {
    for (const set of [light, dark]) {
      expect(new Set(TAG_COLORS.map((c) => set[`tag-${c}`].toLowerCase())).size).toBe(8);
    }
  });

  it("are mapped by the stylesheet: each name reads its own token, and the chip is drawn from it", () => {
    for (const color of TAG_COLORS) {
      expect(tagsCss, color).toContain(
        `[data-tag-color="${color}"] {\n  --tag-fg: var(--tag-${color});`,
      );
    }
    expect(tagsCss).toContain("var(--tag-tint)");
  });

  const themes: [string, Record<string, string>][] = [
    ...TONES.map((name): [string, Record<string, string>] => [`light, ${name} tone`, soft(name)]),
    ["dark", { ...light, ...dark }],
  ];

  describe.each(themes)("in the %s", (_name, set) => {
    it("keep 4.5:1 for the name and icon on the tint of the chip", () => {
      const low: string[] = [];
      for (const color of TAG_COLORS) {
        const fg = set[`tag-${color}`];
        const r = ratio(rgb(fg), mixed(fg, tint, set.bg));
        if (r < 4.5) low.push(`${color}: ${r.toFixed(2)}`);
      }
      expect(low).toEqual([]);
    });

    it("keep 4.5:1 for the mark in a list, on every surface it can sit on", () => {
      const low: string[] = [];
      for (const color of TAG_COLORS) {
        for (const surface of ["bg", "surface", "surface-2"]) {
          const r = ratio(rgb(set[`tag-${color}`]), rgb(set[surface]));
          if (r < 4.5) low.push(`${color} on ${surface}: ${r.toFixed(2)}`);
        }
      }
      expect(low).toEqual([]);
    });
  });
});
