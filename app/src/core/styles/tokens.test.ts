import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const css = readFileSync(new URL("./tokens.css", import.meta.url), "utf8");

/** The value of `--<name>` inside the `:root:lang(<lang>)` block. */
function stack(lang: string, name: string): string[] {
  const block = css.split(`:root:lang(${lang})`)[1]?.split("}")[0] ?? "";
  const value = new RegExp(`--${name}:([^;]+);`).exec(block)?.[1] ?? "";
  return value.split(",").map((family) => family.trim().replace(/"/g, ""));
}

/** The custom properties declared in the first block whose selector is `selector`. */
function block(selector: string): Record<string, string> {
  const body = css.replace(/\r\n/g, "\n").split(`${selector} {`)[1]?.split("}")[0] ?? "";
  const props: Record<string, string> = {};
  for (const match of body.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/--([\w-]+):\s*([^;]+);/g))
    props[match[1] ?? ""] = (match[2] ?? "").trim();
  return props;
}

/** A token's color as sRGB channels, following `var(--x)` inside the same theme. */
function rgb(theme: Record<string, string>, name: string): number[] {
  let value = theme[name] ?? "";
  for (let hops = 0; value.startsWith("var(--") && hops < 4; hops++)
    value = theme[value.slice(6, -1)] ?? "";
  const hex = /^#([0-9a-f]{6})$/i.exec(value)?.[1];
  if (hex === undefined) throw new Error(`--${name} is not a #rrggbb color: "${value}"`);
  return [0, 2, 4].map((at) => parseInt(hex.slice(at, at + 2), 16));
}

/** WCAG 2 contrast ratio of two sRGB colors. */
function contrast(a: number[], b: number[]): number {
  const luminance = ([r = 0, g = 0, b = 0]: number[]): number => {
    const lin = (c: number) =>
      c / 255 <= 0.03928 ? c / 255 / 12.92 : ((c / 255 + 0.055) / 1.055) ** 2.4;
    return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
  };
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return ((hi ?? 0) + 0.05) / ((lo ?? 0) + 0.05);
}

describe("text contrast of every theme and light tone", () => {
  const light = block(':root,\n[data-theme="light"]');
  const themes: Record<string, Record<string, string>> = {
    light,
    dark: block('[data-theme="dark"]'),
  };
  for (const tone of ["paper", "sage", "gray"])
    themes[tone] = { ...light, ...block(`:root[data-tone="${tone}"]:not([data-theme="dark"])`) };

  for (const [name, theme] of Object.entries(themes)) {
    it(`${name}: text, muted, faint and accent text keep 4.5:1 on the page, both surfaces and menus`, () => {
      for (const bg of ["bg", "surface", "surface-2", "pop"])
        for (const fg of ["text", "muted", "faint", "accent-text"])
          expect(
            contrast(rgb(theme, fg), rgb(theme, bg)),
            `--${fg} on --${bg}`,
          ).toBeGreaterThanOrEqual(4.5);
      expect(
        contrast(rgb(theme, "accent-fg"), rgb(theme, "accent")),
        "--accent-fg on --accent",
      ).toBeGreaterThanOrEqual(4.5);
    });

    // diagrams draw chart marks (slices, bars) in the tag colors and write on them in the page color; a gantt task is
    // a 35 % tint of its tag color over the page, with the page's text on it
    it(`${name}: a chart mark shows on the page (3:1) and the page color reads on it (4.5:1)`, () => {
      for (const tag of ["gray", "red", "orange", "yellow", "green", "teal", "blue", "purple"]) {
        const mark = rgb(theme, `tag-${tag}`);
        const page = rgb(theme, "bg");
        const bar = mark.map((c, i) => Math.round(c * 0.35 + (page[i] ?? 0) * 0.65));
        expect(
          contrast(rgb(theme, "text"), bar),
          `--text on a --tag-${tag} bar`,
        ).toBeGreaterThanOrEqual(4.5);
        for (const bg of ["bg", "surface"])
          expect(contrast(mark, rgb(theme, bg)), `--tag-${tag} on --${bg}`).toBeGreaterThanOrEqual(
            3,
          );
        expect(contrast(rgb(theme, "bg"), mark), `--bg on --tag-${tag}`).toBeGreaterThanOrEqual(
          4.5,
        );
      }
    });
  }
});

describe("CJK font stacks", () => {
  // system-ui can be a font with Chinese Han shapes (Linux): the language's own fonts must come first
  for (const [lang, own] of [
    ["ja", ["Yu Gothic UI", "Meiryo UI", "Hiragino Sans", "Noto Sans CJK JP"]],
    ["ko", ["Malgun Gothic", "Apple SD Gothic Neo", "Noto Sans CJK KR"]],
  ] as const) {
    for (const name of ["font-ui", "font-reading"]) {
      it(`${lang} --${name}: Inter, then the ${lang} fonts, before system-ui and the Chinese ones`, () => {
        const families = stack(lang, name);
        expect(families[0]).toBe("Inter Variable");
        const system = families.indexOf("system-ui");
        expect(system).toBeGreaterThan(0);
        for (const family of own) {
          const at = families.indexOf(family);
          expect(at, family).toBeGreaterThan(0);
          expect(at, family).toBeLessThan(system);
          expect(at, family).toBeLessThan(families.indexOf("Microsoft YaHei UI"));
        }
      });
    }
  }
});
