// @vitest-environment jsdom
import "../../core/editing/visual/jsdomShims";
import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createCore, type Core } from "../../core/editing/visual/core";
import type { ContributionInput } from "../../core/plugins/registry.svelte";
import CodeBlock from "./CodeBlock.svelte";
import { codeSpans } from "./colors";
import { MAX_HIGHLIGHT_CHARS } from "./highlighter";
import plugin from "./index";

let component: Record<string, unknown> | null = null;
let core: Core | null = null;

afterEach(async () => {
  if (component) void unmount(component);
  component = null;
  await core?.destroy();
  core = null;
  document.body.replaceChildren();
});

/** For each character of `code`'s text: the classes of the elements around it, innermost first. */
function classChains(code: Element): string[] {
  const chains: string[] = [];
  const walker = document.createTreeWalker(code, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    const chain: string[] = [];
    for (let at = node.parentElement; at !== null && at !== code; at = at.parentElement) {
      const className = at.getAttribute("class");
      if (className) chain.push(className);
    }
    for (let i = 0; i < (node.textContent ?? "").length; i++) chains.push(chain.join(" < "));
  }
  return chains;
}

async function inView(source: string, info: string): Promise<Element> {
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(CodeBlock, { target, props: { source, info, display: false } });
  await vi.waitFor(() => expect(target.querySelector("pre code [class]")).not.toBeNull());
  const code = target.querySelector("pre code");
  if (code === null) throw new Error("no code");
  return code;
}

async function inEdit(source: string, info: string): Promise<Element> {
  const target = document.createElement("div");
  document.body.append(target);
  core = await createCore({
    host: target,
    code: { label: () => null, colors: () => Promise.resolve(codeSpans) },
  });
  core.setDoc(core.parse(`\`\`\`${info}\n${source}\n\`\`\`\n`));
  await vi.waitFor(() =>
    expect(core?.view.dom.querySelector(".code-block code [class]")).not.toBeNull(),
  );
  const code = core.view.dom.querySelector(".code-block code");
  if (code === null) throw new Error("no code");
  return code;
}

describe("code colors for Edit", () => {
  it("puts the same classes around every character as View, nested tokens included", async () => {
    const samples: [string, string][] = [
      // a directive holding a keyword and a string: the inner token's color wins in View
      ["c", '#include <stdio.h>\n/* main */\nint main(void) { return printf("%d", 1); }'],
      ["js", "const name = `hi ${user.name}`; // greet\nclass A extends B {}"],
      ["yaml", "name: demo\nlist:\n  - 1\n  - 'two'"],
    ];
    for (const [info, source] of samples) {
      const view = classChains(await inView(source, info));
      const edit = classChains(await inEdit(source, info));
      expect(edit, info).toEqual(view);
      expect(new Set(view).size, info).toBeGreaterThan(2);
      if (component) void unmount(component);
      component = null;
      await core?.destroy();
      core = null;
    }
  });

  it("reads View's highlighting as runs of the text with the classes around each, outermost first", async () => {
    const spans = await codeSpans("let a = 1;", "js");
    expect(spans).toEqual([
      { from: 0, to: 3, classes: ["hljs-keyword"] },
      { from: 8, to: 9, classes: ["hljs-number"] },
    ]);
    expect(await codeSpans("#include <a.h>", "c")).toEqual([
      { from: 0, to: 1, classes: ["hljs-meta"] },
      { from: 1, to: 8, classes: ["hljs-meta", "hljs-keyword"] },
      { from: 8, to: 9, classes: ["hljs-meta"] },
      { from: 9, to: 14, classes: ["hljs-meta", "hljs-string"] },
    ]);
  });

  it("leaves plain what View leaves plain: plain-text names and text past the size limit", async () => {
    expect(await codeSpans("let a = 1;", "text")).toBeNull();
    expect(await codeSpans(`let a = 1;\n${"x".repeat(MAX_HIGHLIGHT_CHARS)}`, "js")).toBeNull();
  });

  it("offers the colors with the code embed, and takes them away when the plugin goes", async () => {
    const contributed: ContributionInput[] = [];
    let removed = 0;
    const deactivate = plugin.activate({
      id: "highlight",
      contribute: (contribution) => {
        contributed.push(contribution);
        return () => void (removed += 1);
      },
    });
    const embed = contributed.find((c) => c.kind === "embed");
    if (embed?.kind !== "embed" || embed.colors === undefined) throw new Error("no colors");
    const colors = await embed.colors();
    expect(await colors("let a = 1;", "js")).toEqual(await codeSpans("let a = 1;", "js"));
    deactivate();
    expect(removed).toBe(contributed.length);
  });
});
