import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { TONES } from "./tone";

// The soft tones of the light theme repaint the surfaces, so the small colours drawn on them are tuned: every text pair keeps
// at least the contrast it needs (4.5:1 for text, 3:1 for the faint labels), the same as on white or better.

const css = readFileSync("src/styles/tokens.css", "utf8");

/** The `--name: value` pairs of the block that starts with `selector`. */
function block(selector: string): Record<string, string> {
  const start = css.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`no block for ${selector}`);
  const body = css.slice(css.indexOf("{", start) + 1, css.indexOf("}", start));
  const tokens: Record<string, string> = {};
  for (const m of body.matchAll(/--([\w-]+):\s*([^;]+);/g)) tokens[m[1]] = m[2].trim();
  return tokens;
}

const white = block(':root,\n[data-theme="light"]');
const tone = (name: string): Record<string, string> => ({
  ...white,
  ...block(`:root[data-tone="${name}"]:not([data-theme="dark"])`),
});

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
function ratio(a: string, b: string): number {
  const [hi, lo] = [luminance(rgb(a)), luminance(rgb(b))].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}
/** `fg` at `alpha` over `bg`, as a hex colour. */
function over(fg: string, alpha: number, bg: string): string {
  const mixed = rgb(fg).map((c, i) => Math.round(c * alpha + rgb(bg)[i] * (1 - alpha)));
  return `#${mixed.map((v) => v.toString(16).padStart(2, "0")).join("")}`;
}

/** The tones that repaint the light theme: white is the light theme itself. */
const SOFT = TONES.filter((name) => name !== "white");

const TEXT = [
  "text",
  "muted",
  "accent-text",
  "git-modified-text",
  "git-added-text",
  "git-deleted-text",
  "git-renamed-text",
];
const SURFACES = ["bg", "surface", "surface-2", "zebra"];
const SYNTAX = ["syn-comment", "syn-keyword", "syn-string", "syn-type", "syn-prop", "text"];

describe.each(SOFT)("the %s tone of the light theme", (name) => {
  const t = tone(name);

  it("is complete: every token it paints is a plain colour", () => {
    for (const key of [...TEXT, ...SYNTAX, "faint", ...SURFACES, "border", "border-strong"]) {
      expect(t[key], key).toMatch(/^#[0-9a-f]{6}$/i);
    }
  });

  it("keeps 4.5:1 for text on every surface", () => {
    const low: string[] = [];
    for (const token of TEXT) {
      for (const surface of SURFACES) {
        const r = ratio(t[token], t[surface]);
        if (r < 4.5) low.push(`${token} on ${surface}: ${r.toFixed(2)}`);
      }
    }
    expect(low).toEqual([]);
  });

  it("keeps 3:1 for the faint labels", () => {
    const low = ["bg", "surface", "surface-2"].filter((s) => ratio(t.faint, t[s]) < 3);
    expect(low).toEqual([]);
  });

  it("keeps 4.5:1 for code colours on the code block and on inline code", () => {
    const low: string[] = [];
    for (const token of SYNTAX) {
      for (const surface of ["surface", "surface-2"]) {
        const r = ratio(t[token], t[surface]);
        if (r < 4.5) low.push(`${token} on ${surface}: ${r.toFixed(2)}`);
      }
    }
    expect(low).toEqual([]);
  });

  it("keeps 4.5:1 for the Note title (the accent shade for text) on its tint of the page", () => {
    expect(ratio(t["accent-text"], over(t.accent, 0.07, t.bg))).toBeGreaterThanOrEqual(4.5);
  });

  it("keeps 4.5:1 for the alert titles on their own tint of the page", () => {
    const low: string[] = [];
    for (const token of [
      "git-added-text",
      "git-renamed-text",
      "git-modified-text",
      "git-deleted-text",
    ]) {
      const r = ratio(t[token], over(t[token], 0.07, t.bg));
      if (r < 4.5) low.push(`${token}: ${r.toFixed(2)}`);
    }
    expect(low).toEqual([]);
  });

  it("keeps the search marks readable under the text", () => {
    expect(ratio(t.text, t.mark)).toBeGreaterThanOrEqual(4.5);
    expect(ratio(t.text, t["mark-strong"])).toBeGreaterThanOrEqual(4.5);
  });

  it("is a tone of its own: softer than white, and not the same as another tone", () => {
    expect(t.bg).not.toBe(white.bg);
    for (const other of SOFT.filter((o) => o !== name)) {
      expect(t.bg).not.toBe(tone(other).bg);
    }
  });
});

describe("the white tone", () => {
  it("is the plain light theme: it overrides nothing", () => {
    expect(css).not.toContain(':root[data-tone="white"]');
  });
});
