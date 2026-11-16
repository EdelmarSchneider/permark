// @vitest-environment jsdom
import "./jsdomShims";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createCore, type Core } from "./core";

let core: Core;

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.append(host);
  core = await createCore({ host });
});

afterAll(async () => {
  await core.destroy();
});

const TEXT = [
  "# שלום עולם",
  "",
  "مرحبا بالعالم",
  "",
  "Hello world",
  "",
  "- עברית",
  "- item",
  "",
  "> اقتباس",
  "",
  "| עמודה |",
  "| --- |",
  "| خلية |",
  "",
  "```",
  "let x = 1;",
  "```",
  "",
].join("\n");

describe("text direction in Edit", () => {
  it("lets every text block read its own direction, as View does, and writes nothing of it", () => {
    core.setDoc(core.parse(TEXT));
    const dom = core.view.dom;
    const blocks = [...dom.querySelectorAll("h1, p")];
    // the heading, two paragraphs, two list items, the quote, the two cells
    // (Edit draws a heading's marks before its text)
    expect(blocks.map((block) => block.textContent?.replace(/^#+\s*/, ""))).toEqual([
      "שלום עולם",
      "مرحبا بالعالم",
      "Hello world",
      "עברית",
      "item",
      "اقتباس",
      "עמודה",
      "خلية",
    ]);
    for (const block of blocks)
      expect(block.getAttribute("dir"), block.textContent ?? "").toBe("auto");
    // containers hold no `dir` (it would hide their text from an `auto` around them); code keeps the page's direction
    for (const container of dom.querySelectorAll("ul, li, blockquote, pre, code")) {
      expect(container.hasAttribute("dir"), container.tagName).toBe(false);
    }
    expect(core.serialize(core.view.state.doc)).not.toContain("dir");
  });

  it("lays a list | quote out by its first text block, in both views (the reading styles)", () => {
    const css = readFileSync(join(process.cwd(), "src/core/doc/markdown.css"), "utf8");
    const rule = (selector: string): string =>
      css.split(selector)[1]?.split("}")[0]?.replace(/\s+/g, " ") ?? "";
    // View: `li[dir=auto]` (tight) | `li > p[dir=auto]` (loose); Edit: `li > div > p[dir=auto]`
    for (const first of ["li:first-child", "li:first-child > p:first-child"]) {
      expect(css).toContain(`.md :is(ul, ol):has(> ${first}:dir(rtl)),`);
    }
    expect(rule(".md :is(ul, ol):has(> li:first-child > div > p:first-child:dir(rtl))")).toContain(
      "direction: rtl",
    );
    expect(rule(".md blockquote:has(> p:first-child:dir(rtl))")).toContain("direction: rtl");
    for (const dir of ["rtl", "ltr"]) {
      expect(
        rule(`.md li:has(> p:first-child:dir(${dir}), > div > p:first-child:dir(${dir}))`),
      ).toContain(`direction: ${dir}`);
    }
  });
});
