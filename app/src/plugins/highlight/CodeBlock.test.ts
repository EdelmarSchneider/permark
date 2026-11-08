// @vitest-environment jsdom
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

function show(source: string, info = "yaml"): HTMLElement {
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(CodeBlock, { target, props: { source, info, display: false } });
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

  it("highlights plain code", async () => {
    const target = show("let ideas = 1;\n", "js");
    await vi.waitFor(() =>
      expect(target.querySelector("pre code")?.textContent).toContain("ideas"),
    );
  });
});
