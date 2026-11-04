// @vitest-environment jsdom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import StructuredText from "./StructuredText.svelte";
import type { TreeNode } from "./tree";

const key = 'a\\n"b';
const decoded = 'a\n"b';
const spelling = JSON.stringify(decoded);
const text = '{"a\\\\n\\"b":"a\\n\\"b","n":1.50}';

const root: TreeNode = {
  label: null,
  kind: "object",
  summary: "2 keys",
  children: () => [
    { label: key, kind: "string", summary: spelling, children: null },
    { label: "n", kind: "number", summary: "1.50", children: null },
  ],
};

let target: HTMLElement;
let component: Record<string, unknown> | null = null;

function show(source: string, strings = false, ok = true): void {
  target = document.createElement("div");
  document.body.append(target);
  component = mount(StructuredText, {
    target,
    props: {
      text: source,
      parse: () => (ok ? { ok: true as const, root } : { ok: false as const }),
      ...(strings ? { strings: true } : {}),
    },
  });
  flushSync();
}

function interpret(): HTMLButtonElement | null {
  const found = [...target.querySelectorAll("button")].find(
    (button) => button.textContent === "Interpret escapes",
  );
  return found instanceof HTMLButtonElement ? found : null;
}

function summary(kind: string): string {
  const el = target.querySelector(`.kind-${kind}`);
  if (!(el instanceof HTMLElement)) throw new Error(kind);
  return el.textContent ?? "";
}

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  document.body.replaceChildren();
});

describe("StructuredText strings", () => {
  it("has no interpret button unless strings is set", () => {
    show(text);
    expect(interpret()).toBeNull();
    expect(summary("string")).toBe(spelling);
  });

  it("has no interpret button when the text does not parse", () => {
    show("{", true, false);
    expect(interpret()).toBeNull();
    expect(target.querySelector(".source")?.textContent).toBe("{");
  });

  it("decodes a string on click, restores the spelling on the next click, and leaves the key", () => {
    show(text, true);
    const button = interpret();
    if (!button) throw new Error("no button");
    expect(button.getAttribute("aria-pressed")).toBe("false");
    expect(button.textContent).toBe("Interpret escapes");
    expect(summary("string")).toBe(spelling);
    expect(summary("number")).toBe("1.50");
    expect(target.querySelector(".label")?.textContent).toBe(key);

    button.click();
    flushSync();
    expect(interpret()?.getAttribute("aria-pressed")).toBe("true");
    expect(interpret()?.textContent).toBe("Interpret escapes");
    expect(summary("string")).toBe(decoded);
    expect(summary("string").startsWith('"')).toBe(false);
    expect(summary("string")).toContain("\n");
    expect(summary("number")).toBe("1.50");
    expect(target.querySelector(".label")?.textContent).toBe(key);

    interpret()?.click();
    flushSync();
    expect(interpret()?.getAttribute("aria-pressed")).toBe("false");
    expect(summary("string")).toBe(spelling);
    expect(target.querySelector(".label")?.textContent).toBe(key);

    interpret()?.click();
    flushSync();
    const source = [...target.querySelectorAll("button")].find((button) => button.textContent === "Source");
    source?.click();
    flushSync();
    expect(target.querySelector(".source")?.textContent).toBe(text);
    expect(target.querySelector(".source")?.textContent).toContain("\\n");
    expect(interpret()?.getAttribute("aria-pressed")).toBe("true");
    const tree = [...target.querySelectorAll("button")].find((button) => button.textContent === "Tree");
    tree?.click();
    flushSync();
    expect(summary("string")).toBe(decoded);
    expect(target.querySelector(".label")?.textContent).toBe(key);
  });
});
