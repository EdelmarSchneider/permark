// @vitest-environment jsdom
// The table menu of the Edit toolbar: rows and columns go on either side of the caret's cell; no row goes above a
// table's header row.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import EditorToolbar from "./EditorToolbar.svelte";
import type { Command, Format } from "./visual/commands";

let component: Record<string, unknown> | null = null;

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
});

function format(table: boolean, tableHeader: boolean): Format {
  return {
    block: "paragraph",
    strong: false,
    emphasis: false,
    strike: false,
    code: false,
    link: null,
    list: null,
    table,
    tableHeader,
    locked: false,
    language: null,
  };
}

/** Mounts the toolbar, opens its table menu and returns the menu's items by label. */
function tableMenu(shown: Format, run: (command: Command) => void): Map<string, HTMLButtonElement> {
  const target = document.createElement("div");
  document.body.append(target);
  const noop = (): void => {};
  component = mount(EditorToolbar, {
    target,
    props: { format: shown, run, link: noop, image: noop, emoji: noop, symbols: noop },
  });
  flushSync();
  target.querySelector<HTMLButtonElement>("button[aria-label='Table']")?.click();
  flushSync();
  const items = target.querySelectorAll<HTMLButtonElement>("[role=menu] [role=menuitem]");
  return new Map([...items].map((item) => [item.textContent?.trim() ?? "", item]));
}

describe("Edit toolbar table menu", () => {
  it("offers a row above and a column to the left in a body cell, each next to its after-command", () => {
    const run = vi.fn();
    const items = tableMenu(format(true, false), run);
    expect([...items.keys()]).toEqual([
      "Insert table (3 x 3)",
      "Add row above",
      "Add row below",
      "Add column to the left",
      "Add column to the right",
      "Delete row",
      "Delete column",
      "Delete table",
    ]);
    expect(items.get("Add row above")?.disabled).toBe(false);
    items.get("Add row above")?.click();
    flushSync();
    expect(run).toHaveBeenLastCalledWith({ kind: "addRowAbove" });
  });

  it("keeps 'Add row above' off in the header row; a column to the left stays on", () => {
    const run = vi.fn();
    const items = tableMenu(format(true, true), run);
    expect(items.get("Add row above")?.disabled).toBe(true);
    expect(items.get("Add column to the left")?.disabled).toBe(false);
    items.get("Add column to the left")?.click();
    flushSync();
    expect(run).toHaveBeenLastCalledWith({ kind: "addColumnLeft" });
  });

  it("outside a table both are off", () => {
    const items = tableMenu(format(false, false), vi.fn());
    expect(items.get("Add row above")?.disabled).toBe(true);
    expect(items.get("Add column to the left")?.disabled).toBe(true);
  });
});
