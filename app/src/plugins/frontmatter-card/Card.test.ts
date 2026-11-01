// @vitest-environment jsdom
// View front matter: one read-only row per field. Vitest drops Svelte CSS, so the card's style is installed
// and the mounted fields are measured from it.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { language, t } from "../../core/i18n";
import type { FrontMatterProps } from "../../core/plugins/props";
import Card from "./Card.svelte";

const LONG = `Folded description ${"x".repeat(80)} of a SKILL.md`;
const DATE = "2026-10-19";

const PROPS = {
  syntax: "yaml",
  raw: `---\nname: PerMark\ndescription: ${LONG}\n---`,
  fields: [
    { key: "name", value: { type: "text", text: "PerMark" } },
    { key: "description", value: { type: "text", text: LONG } },
    { key: "tags", value: { type: "list", items: ["view", "edit"] } },
    { key: "updated", value: { type: "text", text: DATE } },
  ],
} satisfies FrontMatterProps;

let component: Record<string, unknown> | null = null;

function cardStyle(): string {
  // Not `new URL("./Card.svelte", import.meta.url)`: Vite turns that into an http URL under jsdom.
  const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "Card.svelte"), "utf8");
  const start = source.indexOf("<style>");
  const end = source.lastIndexOf("</style>");
  if (start < 0 || end < start) throw new Error("Card.svelte has no style");
  return source.slice(start + "<style>".length, end);
}

function installStyle(): void {
  document.querySelector("style[data-card-style]")?.remove();
  const style = document.createElement("style");
  style.setAttribute("data-card-style", "");
  style.textContent = cardStyle();
  document.head.append(style);
}

function cardSheet(): CSSStyleSheet {
  const node = document.querySelector("style[data-card-style]");
  if (!node) throw new Error("card style is missing");
  for (const sheet of document.styleSheets) {
    if (sheet.ownerNode === node) return sheet;
  }
  throw new Error("card style was not parsed");
}

function isMediaRule(rule: CSSRule): rule is CSSMediaRule {
  return rule.type === CSSRule.MEDIA_RULE;
}

function isStyleRule(rule: CSSRule): rule is CSSStyleRule {
  return rule.type === CSSRule.STYLE_RULE;
}

type Decl = { media: string | null; selector: string; style: CSSStyleDeclaration };

function declarations(): Decl[] {
  const out: Decl[] = [];
  const visit = (list: CSSRuleList, media: string | null): void => {
    for (const rule of list) {
      if (isMediaRule(rule)) visit(rule.cssRules, rule.conditionText);
      else if (isStyleRule(rule)) {
        out.push({ media, selector: rule.selectorText, style: rule.style });
      }
    }
  };
  visit(cardSheet().cssRules, null);
  return out;
}

function declared(selector: string, media: string | null = null): CSSStyleDeclaration {
  const found = declarations().filter((item) => item.selector === selector && item.media === media);
  if (found.length !== 1)
    throw new Error(`${selector} @ ${media ?? "root"} matched ${found.length}`);
  const item = found[0];
  if (!item) throw new Error(`missing ${selector}`);
  return item.style;
}

/** Top-level grid tracks (`minmax(110px, 26%)` stays one track). */
function tracks(columns: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let start = 0;
  const text = columns.trim();
  for (let i = 0; i <= text.length; i += 1) {
    const ch = text[i];
    if (ch === "(") depth += 1;
    else if (ch === ")") depth -= 1;
    else if ((ch === undefined || /\s/.test(ch)) && depth === 0) {
      const piece = text.slice(start, i).replace(/\s+/g, " ").trim();
      if (piece) out.push(piece);
      start = i + 1;
    }
  }
  return out;
}

function mountCard(props: FrontMatterProps): HTMLElement {
  if (component) unmount(component);
  document.body.replaceChildren();
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(Card, { target, props });
  flushSync();
  return target;
}

function field(root: ParentNode, key: string): HTMLElement {
  const found = [...root.querySelectorAll<HTMLElement>(".field")].find(
    (row) => row.querySelector("dt")?.textContent === key,
  );
  if (!found) throw new Error(`no field ${key}`);
  return found;
}

beforeEach(async () => {
  await language.choose("en", []);
  installStyle();
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  document.body.replaceChildren();
  document.querySelector("style[data-card-style]")?.remove();
});

describe("the front matter card", () => {
  it("lays each field on one row instead of an auto-fit grid", () => {
    const root = mountCard(PROPS);
    const grid = root.querySelector(".grid");
    if (!grid) throw new Error("no grid");
    // One column of rows, not `repeat(auto-fit, minmax(140px, 1fr))`.
    expect(getComputedStyle(grid).gridTemplateColumns).toBe("minmax(0, 1fr)");

    const rows = [...root.querySelectorAll<HTMLElement>(".field")];
    expect(rows).toHaveLength(PROPS.fields.length);
    for (const row of rows) {
      expect([...row.children].map((child) => child.tagName)).toEqual(["DT", "DD"]);
      expect(tracks(getComputedStyle(row).gridTemplateColumns)).toEqual([
        "minmax(110px, 26%)",
        "minmax(0, 1fr)",
      ]);
      expect(getComputedStyle(row).display).toBe("grid");
    }
    expect(root.querySelector(".tools")).toBeNull();

    const narrow = "(max-width: 560px)";
    expect(tracks(declared(".field", narrow).getPropertyValue("grid-template-columns"))).toEqual([
      "minmax(0, 1fr)",
    ]);
    expect(declared("dt").getPropertyValue("color")).toBe("var(--muted)");
    expect(declared("dt").getPropertyValue("font-family")).toBe("var(--font-code)");
    expect(declared("dd").getPropertyValue("min-width")).toBe("0px");
    expect(declared("dd").getPropertyValue("overflow-wrap")).toBe("anywhere");
    expect(declared(".card").getPropertyValue("border")).toBe("1px solid var(--border)");
    expect(declared(".card").getPropertyValue("border-radius")).toBe("var(--radius-card)");
    expect(declared(".card").getPropertyValue("background")).toBe("var(--surface)");
    expect(declarations().some((item) => item.selector.includes("tools"))).toBe(false);
  });

  it("shows each field in order, a list as chips, and a date for the reader", () => {
    for (const syntax of ["yaml", "toml"] as const) {
      const root = mountCard({ ...PROPS, syntax });
      expect(root.querySelector("section.card")?.getAttribute("aria-label")).toBe(
        t("doc.frontmatter"),
      );
      expect([...root.querySelectorAll(".field dt")].map((node) => node.textContent)).toEqual([
        "name",
        "description",
        "tags",
        "updated",
      ]);
      expect(field(root, "name").querySelector("dd")?.textContent).toBe("PerMark");
      expect(field(root, "description").querySelector("dd")?.textContent).toBe(LONG);
      expect(
        [...field(root, "tags").querySelectorAll(".chip")].map((chip) => chip.textContent),
      ).toEqual(["view", "edit"]);
      expect(field(root, "updated").querySelector("dd")?.textContent).toBe(
        new Date(`${DATE}T00:00:00`).toLocaleDateString(language.locale, { dateStyle: "long" }),
      );
      expect(root.querySelector("pre.raw")).toBeNull();
    }
    expect(declared(".chips").getPropertyValue("flex-wrap")).toBe("wrap");
    expect(declared(".chips").getPropertyValue("display")).toBe("flex");
  });

  it("shows front matter it cannot read as the raw block", () => {
    const yaml = mountCard({ syntax: "yaml", raw: "\n- just\n- a list\n", fields: [] });
    expect(yaml.querySelector("pre.raw")?.textContent).toBe("- just\n- a list");
    expect(yaml.querySelector("pre.raw")?.getAttribute("data-syntax")).toBe("yaml");
    expect(yaml.querySelector(".field, .grid")).toBeNull();

    const toml = mountCard({ syntax: "toml", raw: '\n[[item]]\nname = "x"\n', fields: [] });
    expect(toml.querySelector("pre.raw")?.textContent).toBe('[[item]]\nname = "x"');
    expect(toml.querySelector("pre.raw")?.getAttribute("data-syntax")).toBe("toml");
    expect(toml.querySelector(".field, .grid")).toBeNull();
  });

  it("stays read-only", () => {
    const root = mountCard(PROPS);
    expect(
      root.querySelector("input, textarea, button, select, [contenteditable='true'], .tools"),
    ).toBeNull();
  });
});
