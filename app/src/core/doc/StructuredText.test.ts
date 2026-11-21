// @vitest-environment jsdom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import StructuredText from "./StructuredText.svelte";
import { CHUNK, type TreeNode } from "./tree";

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

function show(source: string, strings = false, ok = true, tree: TreeNode = root): void {
  target = document.createElement("div");
  document.body.append(target);
  component = mount(StructuredText, {
    target,
    props: {
      text: source,
      parse: () => (ok ? { ok: true as const, root: tree } : { ok: false as const }),
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
    const source = [...target.querySelectorAll("button")].find(
      (button) => button.textContent === "Source",
    );
    source?.click();
    flushSync();
    expect(target.querySelector(".source")?.textContent).toBe(text);
    expect(target.querySelector(".source")?.textContent).toContain("\\n");
    expect(interpret()?.getAttribute("aria-pressed")).toBe("true");
    const tree = [...target.querySelectorAll("button")].find(
      (button) => button.textContent === "Tree",
    );
    tree?.click();
    flushSync();
    expect(summary("string")).toBe(decoded);
    expect(target.querySelector(".label")?.textContent).toBe(key);
  });
});

/** An object `levels` deep with `width` keys per level. */
function nested(width: number, levels: number, label: string | null = null): TreeNode {
  if (levels === 0) return { label, kind: "number", summary: "1", children: null };
  return {
    label,
    kind: "object",
    summary: `${width} keys`,
    children: () => Array.from({ length: width }, (_, at) => nested(width, levels - 1, `k${at}`)),
  };
}

function button(name: string): HTMLButtonElement | null {
  const found = [...target.querySelectorAll("button")].find(
    (el) => el.textContent?.trim() === name,
  );
  return found instanceof HTMLButtonElement ? found : null;
}

function press(name: string): void {
  const found = button(name);
  if (!found) throw new Error(name);
  found.click();
  flushSync();
}

/** aria-expanded of every branch row, in document order. */
function heads(): string[] {
  return [...target.querySelectorAll(".head[aria-expanded]")].map(
    (el) => el.getAttribute("aria-expanded") ?? "",
  );
}

describe("StructuredText roles", () => {
  it("names the tree as a group of disclosure rows, and Tree | Source as pressed toggles", () => {
    show(text);
    // rows are buttons that open and close (aria-expanded), not tree items: no tree role without them
    expect(target.querySelector("[role=tree]")).toBeNull();
    const tree = target.querySelector(".tree");
    expect(tree?.getAttribute("role")).toBe("group");
    expect(tree?.getAttribute("aria-label")).toBe("Tree");
    // two toggles, no radio group without arrow keys
    expect(target.querySelector("[role=radiogroup], [role=radio]")).toBeNull();
    const modes = target.querySelector(".segments");
    expect(modes?.getAttribute("role")).toBe("group");
    // not "View": that is the name of the document's View | Edit mode button beside it
    expect(modes?.getAttribute("aria-label")).toBe("Display");
    const pressed = (): (string | null)[] =>
      [...target.querySelectorAll(".segment")].map((el) => el.getAttribute("aria-pressed"));
    expect(pressed()).toEqual(["true", "false"]);
    press("Source");
    expect(pressed()).toEqual(["false", "true"]);
    expect(target.querySelector(".tree")).toBeNull();
    press("Tree");
    expect(pressed()).toEqual(["true", "false"]);
  });
});

describe("StructuredText expand and collapse all", () => {
  it("offers both in Tree, also without the interpret button, and not in Source", () => {
    show(text);
    expect(button("Expand all")?.type).toBe("button");
    expect(button("Collapse all")?.type).toBe("button");
    press("Source");
    expect(button("Expand all")).toBeNull();
    expect(button("Collapse all")).toBeNull();
    press("Tree");
    expect(button("Expand all")).not.toBeNull();
  });

  it("is absent when the text does not parse", () => {
    show("{", true, false);
    expect(button("Expand all")).toBeNull();
    expect(button("Collapse all")).toBeNull();
  });

  it("opens a small document fully", () => {
    show("{}", false, true, nested(3, 3));
    // 1 + 3 + 9 branch rows, all open
    expect(heads()).toHaveLength(13);
    expect(heads().every((state) => state === "true")).toBe(true);
  });

  it("opens a big document at its first level only", () => {
    show("{}", false, true, nested(CHUNK, 2));
    const states = heads();
    expect(states[0]).toBe("true");
    expect(states).toHaveLength(CHUNK + 1);
    expect(states.slice(1).every((state) => state === "false")).toBe(true);
  });

  it("collapses to the root, expands again, and keeps row toggles working after each", () => {
    show("{}", false, true, nested(3, 3));
    press("Collapse all");
    expect(heads()).toEqual(["true", "false", "false", "false"]);

    const rows = (): HTMLButtonElement[] => [
      ...target.querySelectorAll<HTMLButtonElement>(".head[aria-expanded]"),
    ];
    rows()[1]?.click();
    flushSync();
    expect(heads()).toEqual(["true", "true", "false", "false", "false", "false", "false"]);

    press("Expand all");
    expect(heads()).toHaveLength(13);
    expect(heads().every((state) => state === "true")).toBe(true);

    rows()[1]?.click();
    flushSync();
    expect(heads()).toHaveLength(10);
    expect(heads()[1]).toBe("false");

    // the same ask again applies over the row's own choice
    press("Expand all");
    expect(heads()).toHaveLength(13);
    expect(heads().every((state) => state === "true")).toBe(true);

    press("Collapse all");
    rows()[0]?.click();
    flushSync();
    expect(heads()).toEqual(["false"]);
  });

  it("expands a big document and keeps a long list in chunks", () => {
    show("{}", false, true, nested(CHUNK + 5, 1));
    press("Collapse all");
    press("Expand all");
    expect(target.querySelectorAll(".row .row")).toHaveLength(CHUNK);
    expect(target.querySelector(".more")).not.toBeNull();
  });
});
