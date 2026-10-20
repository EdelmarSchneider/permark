// @vitest-environment jsdom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import CodeBlock from "./CodeBlock.svelte";

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

describe("code blocks with secret values", () => {
  it("never put a secret in the page, highlighted or not, and show each on its own click", async () => {
    const target = show(SOURCE);
    // before the highlighter answers
    for (const secret of ["abc123", "hunter2"]) {
      expect(target.textContent, secret).not.toContain(secret);
    }
    expect(target.querySelectorAll("button.secret")).toHaveLength(2);
    // after it
    await vi.waitFor(() => expect(target.querySelector(".hljs-attr, .hljs-string")).not.toBeNull());
    for (const secret of ["abc123", "hunter2"]) {
      expect(target.textContent, secret).not.toContain(secret);
    }
    expect(target.textContent).toContain("API_KEY");
    expect(target.textContent).toContain("plain");
    expect(target.querySelectorAll("button.secret")).toHaveLength(2);
    target.querySelector<HTMLButtonElement>("button.secret")?.click();
    flushSync();
    await vi.waitFor(() => expect(target.textContent).toContain("abc123"));
    expect(target.textContent).not.toContain("hunter2");
    expect(target.querySelectorAll("button.secret")).toHaveLength(1);
  });

  it("copies the real text", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const target = show(SOURCE);
    target.querySelector<HTMLButtonElement>("button.copy")?.click();
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith(SOURCE));
  });

  it("is the same block as before when there is nothing to hide", async () => {
    const target = show("let ideas = 1;\n", "js");
    await vi.waitFor(() =>
      expect(target.querySelector("pre code")?.textContent).toContain("ideas"),
    );
    expect(target.querySelector("button.secret")).toBeNull();
  });
});
