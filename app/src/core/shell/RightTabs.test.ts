// @vitest-environment jsdom
// One group's tab strip in the right column: labels + count, one Tab stop with arrows, selecting a tab that is not
// selected, the view's options from the keyboard (shell.md §Frame, Right region).
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import RightTabs from "./RightTabs.svelte";

let component: Record<string, unknown> | null = null;

const VIEWS = [
  { id: "outline", label: "Outline", icon: undefined, badge: null },
  { id: "live-feed.panel", label: "Live changes", icon: undefined, badge: "3" },
];

function show(
  selected: string | null,
  onselect = vi.fn(),
  onmenu = vi.fn(),
): { root: HTMLElement; onselect: typeof onselect; onmenu: typeof onmenu } {
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(RightTabs, { target, props: { views: VIEWS, selected, onselect, onmenu } });
  flushSync();
  return { root: target, onselect, onmenu };
}

const tabs = (root: HTMLElement): HTMLButtonElement[] => [
  ...root.querySelectorAll<HTMLButtonElement>('[role="tab"]'),
];

const key = (root: HTMLElement, init: KeyboardEventInit): void => {
  root
    .querySelector('[role="tablist"]')
    ?.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, ...init }));
  flushSync();
};

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
});

describe("a group's tabs", () => {
  it("say their views, the selected one, the count on a tab; the selected one takes the Tab stop", () => {
    const { root } = show("outline");
    const [outline, live] = tabs(root);
    expect(root.querySelector('[role="tablist"]')?.getAttribute("aria-label")).toBe(
      "Right panel views",
    );
    expect(outline?.textContent?.trim()).toBe("Outline");
    expect(outline?.getAttribute("aria-selected")).toBe("true");
    expect(live?.getAttribute("aria-label")).toBe("Live changes (3)");
    expect(live?.querySelector(".badge")?.textContent).toBe("3");
    expect([outline?.tabIndex, live?.tabIndex]).toEqual([0, -1]);
  });

  it("select a tab that is not selected; clicking the selected one changes nothing", () => {
    const { root, onselect } = show("outline");
    tabs(root)[0]?.click();
    expect(onselect).not.toHaveBeenCalled();
    tabs(root)[1]?.click();
    expect(onselect).toHaveBeenCalledWith("live-feed.panel");
  });

  it("move the focus with arrows, Home and End, without selecting", () => {
    const { root, onselect } = show("outline");
    tabs(root)[0]?.focus();
    key(root, { key: "ArrowRight" });
    expect(document.activeElement).toBe(tabs(root)[1]);
    expect(tabs(root)[1]?.tabIndex).toBe(0);
    key(root, { key: "ArrowRight" });
    expect(document.activeElement).toBe(tabs(root)[0]);
    key(root, { key: "End" });
    expect(document.activeElement).toBe(tabs(root)[1]);
    key(root, { key: "Home" });
    expect(document.activeElement).toBe(tabs(root)[0]);
    expect(onselect).not.toHaveBeenCalled();
  });

  it("nothing selected: the first tab keeps the Tab stop", () => {
    const { root } = show(null);
    expect(tabs(root)[0]?.tabIndex).toBe(0);
  });

  it("Shift+F10 | the Menu key open the options of the tab with the Tab stop", () => {
    const { root, onmenu } = show("live-feed.panel");
    key(root, { key: "F10", shiftKey: true });
    expect(onmenu).toHaveBeenCalledTimes(1);
    expect(onmenu.mock.calls[0]?.[0]).toBe("live-feed.panel");
    key(root, { key: "ContextMenu" });
    expect(onmenu).toHaveBeenCalledTimes(2);
  });
});
