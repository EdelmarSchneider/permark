// @vitest-environment jsdom
// The shared context menu (Explorer, right column views): the keyboard focus itself moves over the items (assistive
// tech follows it), a disabled item is reached and announced but does nothing, focus returns to the opener.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import ContextMenu from "./ContextMenu.svelte";
import type { MenuEntry } from "./menu";

let component: Record<string, unknown> | null = null;
let opener: HTMLButtonElement;

function show(entries: MenuEntry[], close = vi.fn()): HTMLElement {
  opener = document.createElement("button");
  document.body.append(opener);
  opener.focus();
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(ContextMenu, {
    target,
    props: { x: 10, y: 10, entries, close, label: "Options" },
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
