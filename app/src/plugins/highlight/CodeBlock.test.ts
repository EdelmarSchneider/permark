// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import CodeBlock from "./CodeBlock.svelte";
import { fenceLabel } from "./languages";

let component: Record<string, unknown> | null = null;

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

const SOURCE = "name: demo\nAPI_KEY: abc123\npassword = 'hunter2'\nplain: 1\n";

function show(
  source: string,
  info = "yaml",
  extras: { title?: string; start?: number; marked?: string } = {},
): HTMLElement {
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(CodeBlock, { target, props: { source, info, display: false, ...extras } });
  return target;
}

describe("code blocks", () => {
  it("show the text as written, highlighted or not", async () => {
    const target = show(SOURCE);
    // before the highlighter answers
    expect(target.querySelector("pre code")?.textContent).toBe(SOURCE);
    // after it
    await vi.waitFor(() => expect(target.querySelector(".hljs-attr, .hljs-string")).not.toBeNull());
    for (const value of ["API_KEY", "abc123", "hunter2", "plain"]) {
      expect(target.textContent, value).toContain(value);
    }
    expect(target.querySelector("pre button")).toBeNull();
  });

  it("labels a fence that names its language as the label Edit mode is given for it", async () => {
    for (const info of ["rust", "JS", "my-dsl"]) {
      const target = show("let a = 1;\n", info);
      await vi.waitFor(() => expect(target.querySelector(".lang")).not.toBeNull());
      expect(target.querySelector(".lang")?.textContent?.trim()).toBe(fenceLabel(info));
      if (component) void unmount(component);
      component = null;
    }
    expect(fenceLabel("rust")).toBe("Rust");
    expect(fenceLabel("my-dsl")).toBe("my-dsl");
    // plain text and no language: no label
    expect(fenceLabel("text")).toBeNull();
    expect(fenceLabel("")).toBeNull();
  });

  it("copies the text", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const target = show(SOURCE);
    target.querySelector<HTMLButtonElement>("button.copy")?.click();
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith(SOURCE));
  });

  it("draws the title, the line numbers and the marked lines the info string asked for", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const source = "let a = 1;\nlet b = 2;\nlet c = 3;\n";
    const target = show(source, "js", { title: "main.js", start: 10, marked: "2" });
    flushSync();
    expect(target.querySelector(".code-title")?.textContent).toBe("main.js");
    const pre = target.querySelector("pre");
    expect(pre?.classList.contains("is-numbered")).toBe(true);
    expect(pre?.style.getPropertyValue("--ln-start")).toBe("9");
    await vi.waitFor(() => expect(target.querySelector(".hljs-keyword")).not.toBeNull());
    const lines = [...target.querySelectorAll("pre .ln")];
    expect(lines).toHaveLength(3);
    expect(lines.map((line) => line.classList.contains("is-marked"))).toEqual([false, true, false]);
    // the numbers are drawn by the style, never text: the code reads (and copies) as written
    expect(target.querySelector("pre code")?.textContent).toBe(source);
    target.querySelector<HTMLButtonElement>("button.copy")?.click();
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith(source));
  });

  it("a wrapped numbered line goes on in the code column, under its own indentation", () => {
    const target = show('fn main() {\n    println!("a long line");\n}\n', "rust", { start: 1 });
    flushSync();
    const lines = [...target.querySelectorAll<HTMLElement>("pre .ln")];
    expect(lines.map((line) => line.style.getPropertyValue("--lead"))).toEqual(["", "4", ""]);
    // the further rows hang right of the number column plus the line's indentation; the number stays put
    const css = readFileSync(
      resolve(process.cwd(), "src/plugins/highlight/CodeBlock.svelte"),
      "utf8",
    );
    const rule = (selector: string): string =>
      new RegExp(`${selector.replace(/[.()[\]:]/g, "\\$&")}\\s*\\{([^}]*)\\}`).exec(css)?.[1] ?? "";
    const line = rule("pre :global(.ln)");
    expect(line).toMatch(/padding-left:\s*calc\(var\(--ln-pad, 0px\) \+ var\(--hang\)\)/);
    expect(line).toMatch(/text-indent:\s*calc\(-1 \* var\(--hang\)\)/);
    expect(line).toMatch(/--hang:\s*calc\(\(2 \+ var\(--lead, 0\)\) \* 1ch\)/);
    expect(rule("pre :global(.ln.is-marked)")).toMatch(/var\(--hang\)/);
    expect(rule("pre.is-numbered :global(.ln::before)")).toMatch(/text-indent:\s*0/);
  });

  it("keeps a block without extras as it was: no title, no numbered lines", () => {
    const target = show("x = 1\n", "py");
    flushSync();
    expect(target.querySelector(".code-title")).toBeNull();
    expect(target.querySelector("pre")?.classList.contains("is-numbered")).toBe(false);
    expect(target.querySelector("pre .ln")).toBeNull();
  });

  it("highlights plain code", async () => {
    const target = show("let ideas = 1;\n", "js");
    await vi.waitFor(() =>
      expect(target.querySelector("pre code")?.textContent).toContain("ideas"),
    );
  });
});
