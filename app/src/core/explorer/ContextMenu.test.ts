// @vitest-environment jsdom
// The shared context menu (Explorer, right column views): the keyboard focus itself moves over the items (assistive
// tech follows it), a disabled item is reached and announced but does nothing, focus returns to the opener.
import FolderOpen from "@lucide/svelte/icons/folder-open";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import ContextMenu from "./ContextMenu.svelte";
import type { MenuEntry } from "./menu";

let component: Record<string, unknown> | null = null;
let opener: HTMLButtonElement;

function show(
  entries: MenuEntry[],
  close = vi.fn(),
  place: { y?: number; above?: boolean } = {},
): HTMLElement {
  opener = document.createElement("button");
  document.body.append(opener);
  opener.focus();
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(ContextMenu, {
    target,
    props: { x: 10, y: 10, entries, close, label: "Options", ...place },
  });
  flushSync();
  return target;
}

const key = (k: string, init: KeyboardEventInit = {}): void => {
  document.activeElement?.dispatchEvent(
    new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...init }),
  );
  flushSync();
};

const focused = (): string | null =>
  document.activeElement instanceof HTMLElement ? document.activeElement.textContent.trim() : null;

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
});

describe("the context menu by keyboard", () => {
  const runs = { up: vi.fn(), down: vi.fn(), close: vi.fn() };
  const entries = (): MenuEntry[] => [
    { kind: "heading", label: "Too short" },
    { kind: "item", label: "Move up", run: runs.up },
    { kind: "item", label: "Move down", disabled: true, run: runs.down },
    { kind: "separator" },
    { kind: "item", label: "Close", run: runs.close },
  ];

  it("puts the focus on the first item, and the arrows move the real focus (wrapping), Home | End too", () => {
    const root = show(entries());
    expect(focused()).toBe("Move up");
    expect(document.activeElement?.getAttribute("role")).toBe("menuitem");
    key("ArrowDown");
    expect(focused()).toBe("Move down");
    key("ArrowDown");
    expect(focused()).toBe("Close");
    key("ArrowDown");
    expect(focused()).toBe("Move up");
    key("ArrowUp");
    expect(focused()).toBe("Close");
    key("Home");
    expect(focused()).toBe("Move up");
    key("End");
    expect(focused()).toBe("Close");
    // one Tab stop: the focused item
    const stops = [...root.querySelectorAll<HTMLElement>('[role="menuitem"]')].filter(
      (el) => el.tabIndex === 0,
    );
    expect(stops.map((el) => el.textContent.trim())).toEqual(["Close"]);
  });

  it("reaches a disabled item, says it is unavailable, and Enter | Space | click do nothing", () => {
    const close = vi.fn();
    show(entries(), close);
    key("ArrowDown");
    expect(focused()).toBe("Move down");
    expect(document.activeElement?.getAttribute("aria-disabled")).toBe("true");
    key("Enter");
    key(" ");
    (document.activeElement as HTMLElement).click();
    flushSync();
    expect(runs.down).not.toHaveBeenCalled();
    expect(close).not.toHaveBeenCalled();
  });

  it("Enter runs the focused item and closes; Escape closes without running", () => {
    const close = vi.fn();
    show(entries(), close);
    key("End");
    key("Enter");
    expect(runs.close).toHaveBeenCalledTimes(1);
    expect(close).toHaveBeenCalledTimes(1);
    key("Escape");
    expect(close).toHaveBeenCalledTimes(2);
  });

  it("gives the focus back to the opener when it goes", () => {
    show(entries());
    expect(document.activeElement).not.toBe(opener);
    if (component) void unmount(component);
    component = null;
    flushSync();
    expect(document.activeElement).toBe(opener);
  });

  it("a menu with no item keeps the focus on itself", () => {
    const root = show([{ kind: "heading", label: "Nothing" }]);
    expect(document.activeElement).toBe(root.querySelector('[role="menu"]'));
    key("ArrowDown");
    expect(document.activeElement).toBe(root.querySelector('[role="menu"]'));
  });
});

describe("icons of the context menu", () => {
  it("draws an item's icon in a leading column, hidden from assistive tech; an item without one keeps the column", () => {
    const root = show([
      { kind: "item", label: "Open folder", icon: FolderOpen, run: vi.fn() },
      { kind: "item", label: "Plain", run: vi.fn() },
    ]);
    const items = [...root.querySelectorAll<HTMLElement>('[role="menuitem"]')];
    const cells = items.map((item) => item.querySelector<HTMLElement>(".icon"));
    expect(cells[0]?.querySelector("svg.lucide-folder-open") ?? null).not.toBeNull();
    expect(cells[0]?.getAttribute("aria-hidden")).toBe("true");
    expect(cells[0]?.querySelector("svg")?.getAttribute("width")).toBe("15");
    // the same column, empty: the labels line up
    expect(cells[1] ?? null).not.toBeNull();
    expect(cells[1]?.querySelector("svg") ?? null).toBeNull();
    // the name stays the label alone
    expect(items.map((item) => item.textContent.trim())).toEqual(["Open folder", "Plain"]);
  });

  it("a menu without icons draws no icon column", () => {
    const root = show([{ kind: "item", label: "Plain", run: vi.fn() }]);
    expect(root.querySelector(".icon")).toBeNull();
  });
});

describe("entries that are on | off on their own", () => {
  const runs = { files: vi.fn(), tags: vi.fn(), wide: vi.fn() };
  const entries = (): MenuEntry[] => [
    { kind: "heading", label: "Sidebar" },
    {
      kind: "item",
      label: "Files",
      checkbox: true,
      checked: true,
      disabled: true,
      why: "Always open",
      run: runs.files,
    },
    { kind: "item", label: "Tags", checkbox: true, checked: false, run: runs.tags },
    { kind: "heading", label: "Bottom panel" },
    { kind: "note", label: "None" },
    { kind: "separator" },
    { kind: "item", label: "Wide", checked: true, run: runs.wide },
  ];

  it("are checkboxes with their state, apart from a choice of a set (a radio)", () => {
    const root = show(entries());
    const roles = [...root.querySelectorAll<HTMLElement>("[data-at]")].map((el) => [
      el.textContent.trim(),
      el.getAttribute("role"),
      el.getAttribute("aria-checked"),
    ]);
    expect(roles).toEqual([
      ["Files", "menuitemcheckbox", "true"],
      ["Tags", "menuitemcheckbox", "false"],
      ["Wide", "menuitemradio", "true"],
    ]);
  });

  it("one that cannot act says why once: its tooltip, which is also its description", () => {
    const root = show(entries());
    const files = root.querySelector<HTMLElement>('[data-at="1"]');
    expect(files?.getAttribute("title")).toBe("Always open");
    // no second copy of the reason for assistive tech to read after the title
    expect(files?.hasAttribute("aria-describedby")).toBe(false);
    expect(root.querySelectorAll("[hidden]")).toHaveLength(0);
    expect(root.querySelector('[data-at="2"]')?.hasAttribute("title")).toBe(false);
  });

  it("a quiet note is passed by the keyboard and does nothing; Space switches the focused entry", () => {
    const close = vi.fn();
    const root = show(entries(), close);
    expect(root.querySelector(".note")?.textContent).toBe("None");
    expect(root.querySelector(".note")?.getAttribute("role")).toBe("presentation");
    // the keyboard starts on the first entry that can act, not the disabled one before it
    expect(focused()).toBe("Tags");
    key("ArrowUp");
    expect(focused()).toBe("Files");
    key("ArrowDown");
    key("ArrowDown");
    // the arrows go over the note to the next entries
    expect(focused()).toBe("Wide");
    key("ArrowUp");
    key(" ");
    expect(runs.tags).toHaveBeenCalledTimes(1);
    // the menu stays for the next switch, the keyboard on the same entry
    expect(close).not.toHaveBeenCalled();
    expect(focused()).toBe("Tags");
  });

  it("a click switches an entry and keeps the menu; Enter switches it and closes the menu", () => {
    const close = vi.fn();
    const root = show(entries(), close);
    const tags = root.querySelector<HTMLElement>('[data-at="2"]');
    tags?.click();
    flushSync();
    expect(runs.tags).toHaveBeenCalledTimes(1);
    expect(close).not.toHaveBeenCalled();
    key("Enter");
    expect(runs.tags).toHaveBeenCalledTimes(2);
    expect(close).toHaveBeenCalledTimes(1);
    // a choice of a set (a radio) closes the menu on a click, as any other item
    root.querySelector<HTMLElement>('[data-at="6"]')?.click();
    flushSync();
    expect(runs.wide).toHaveBeenCalledTimes(1);
    expect(close).toHaveBeenCalledTimes(2);
  });

  it("each heading names the group of the entries under it, up to the next heading | separator", () => {
    const root = show(entries());
    const groups = [...root.querySelectorAll<HTMLElement>('[role="group"]')];
    const named = groups.map((group) => [
      document.getElementById(group.getAttribute("aria-labelledby") ?? "")?.textContent,
      [...group.querySelectorAll("[data-at], .note")].map((el) => el.textContent.trim()),
    ]);
    expect(named).toEqual([
      ["Sidebar", ["Files", "Tags"]],
      ["Bottom panel", ["None"]],
    ]);
    // an entry under no heading stands in the menu itself
    expect(root.querySelector('[data-at="6"]')?.parentElement?.getAttribute("role")).toBe("menu");
  });
});

describe("a menu that opens upward (a control at the bottom of the window)", () => {
  it("is never taller than the room above its control: it scrolls instead of covering the control", () => {
    const root = show([{ kind: "item", label: "One", run: vi.fn() }], vi.fn(), {
      y: 120,
      above: true,
    });
    expect(root.querySelector<HTMLElement>('[role="menu"]')?.style.maxHeight).toBe("116px");
  });
});
