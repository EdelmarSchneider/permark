// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MovePlan } from "../../api";
import { declarations, ruleBody, styleOf } from "../testing/cssRules";
import MovePlanDialog from "./MovePlanDialog.svelte";

const style = styleOf(
  readFileSync(join(process.cwd(), "src/core/explorer/MovePlanDialog.svelte"), "utf8"),
);

let component: Record<string, unknown> | null = null;

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
});

const PLAN: MovePlan = {
  planId: 1,
  moves: [{ from: "a.md", to: "docs/a.md" }],
  linkCount: 1,
  fileCount: 1,
  files: [{ path: "b.md", movedTo: null, changes: [{ line: 3, old: "a.md", new: "docs/a.md" }] }],
  truncated: false,
  skipped: [{ path: "c.md", reason: "tooLarge" }],
  unchecked: [],
};

describe("the move dialog's height", () => {
  it("never grows past the window: the backdrop's one row is the window's height, the dialog fits in it", () => {
    // an auto row grows to the dialog's content, and a percentage height against it caps nothing
    expect(declarations(ruleBody(style, ".backdrop"))).toContain(
      "grid-template-rows: minmax(0, 1fr)",
    );
    expect(declarations(ruleBody(style, ".dialog"))).toContain("max-height: min(640px, 100%)");
  });

  it("scrolls a long list of links or skipped files inside, so the answers stay on screen", () => {
    for (const part of [".list", ".skipped"]) {
      const rules = declarations(ruleBody(style, part));
      expect(rules, part).toContain("min-height: 0");
      expect(rules, part).toContain("overflow: auto");
    }
    expect(declarations(ruleBody(style, "footer"))).toContain("flex-shrink: 0");
  });

  it("keeps a few rows of the files not updated in sight under a long list of links", () => {
    // both lists shrink by their length: a list of thousands of links would leave one row to the other
    const rules = declarations(ruleBody(style, ".list ~ .skipped"));
    expect(rules).toContain("flex-shrink: 0");
    expect(rules.some((rule) => rule.startsWith("max-height:"))).toBe(true);
  });
});

describe("the move dialog and the keyboard", () => {
  function open(returnFocus: () => HTMLElement | null): { answer: ReturnType<typeof vi.fn> } {
    const answer = vi.fn();
    const target = document.createElement("div");
    document.body.append(target);
    component = mount(MovePlanDialog, { target, props: { plan: PLAN, answer, returnFocus } });
    flushSync();
    return { answer };
  }

  it("lets the keyboard reach and scroll both lists, each named", () => {
    open(() => null);
    const links = document.querySelector<HTMLElement>(".list");
    const skipped = document.querySelector<HTMLElement>(".skipped");
    // a named <section> is a region by itself
    expect(links?.getAttribute("role")).toBe("region");
    expect(skipped?.tagName).toBe("SECTION");
    for (const list of [links, skipped]) {
      expect(list?.getAttribute("tabindex")).toBe("0");
      const by = list?.getAttribute("aria-labelledby") ?? "";
      expect(document.getElementById(by)?.textContent?.trim()).not.toBe("");
    }
    expect(
      document.getElementById(skipped?.getAttribute("aria-labelledby") ?? "")?.textContent,
    ).toBe("Not updated");
  });

  it("gives the focus back to the tree after Escape, when what had it (the name field) is gone", async () => {
    const tree = document.createElement("div");
    tree.tabIndex = 0;
    const field = document.createElement("input");
    document.body.append(tree, field);
    field.focus();
    const { answer } = open(() => tree);
    // the name field closes as the dialog takes the focus
    field.remove();
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    expect(answer).toHaveBeenCalledWith("cancel");
    if (component) await unmount(component);
    component = null;
    expect(document.activeElement).toBe(tree);
  });
});
