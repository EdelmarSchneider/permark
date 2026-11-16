import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const css = readFileSync(new URL("./tokens.css", import.meta.url), "utf8");

/** The value of `--<name>` inside the `:root:lang(<lang>)` block. */
function stack(lang: string, name: string): string[] {
  const block = css.split(`:root:lang(${lang})`)[1]?.split("}")[0] ?? "";
  const value = new RegExp(`--${name}:([^;]+);`).exec(block)?.[1] ?? "";
  return value.split(",").map((family) => family.trim().replace(/"/g, ""));
}

describe("bundled Inter subsets", () => {
  const fonts = readFileSync(new URL("./fonts.css", import.meta.url), "utf8");
  const interFiles = [...fonts.matchAll(/inter\/files\/inter-([a-z-]+)-wght-normal\.woff2/g)].map(
    (m) => m[1],
  );

  it("cover the interface languages' scripts, latin declared last so it wins where ranges overlap", () => {
    expect(interFiles).toEqual(["cyrillic-ext", "cyrillic", "vietnamese", "latin-ext", "latin"]);
  });

  it("draw Cyrillic and Vietnamese letters with Inter", () => {
    const ranges = fonts.split("@font-face").filter((face) => face.includes('"Inter Variable"'));
    const covers = (code: number): boolean =>
      ranges.some((face) =>
        [
          ...(/unicode-range:([^;]+);/.exec(face)?.[1] ?? "").matchAll(
            /U\+([0-9A-F]+)(?:-([0-9A-F]+))?/g,
          ),
        ].some(
          ([, from = "", to]) => code >= parseInt(from, 16) && code <= parseInt(to ?? from, 16),
        ),
      );
    for (const letter of ["Ж", "я", "Ґ", "ư", "ộ", "đ", "₫"]) {
      expect(covers(letter.codePointAt(0) ?? 0), letter).toBe(true);
    }
  });
});

describe("CJK font stacks", () => {
  // system-ui can be a font with Chinese Han shapes (Linux): the language's own fonts must come first
  for (const [lang, own] of [
    ["ja", ["Yu Gothic UI", "Meiryo UI", "Hiragino Sans", "Noto Sans CJK JP"]],
    ["ko", ["Malgun Gothic", "Apple SD Gothic Neo", "Noto Sans CJK KR"]],
    ["zh-TW", ["Microsoft JhengHei UI", "PingFang TC", "Noto Sans CJK TC"]],
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
