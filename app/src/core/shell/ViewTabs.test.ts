// @vitest-environment jsdom
// One group's tab strip in the right column: labels + count, one Tab stop with arrows, selecting a tab that is not
// selected, the view's options from the keyboard (shell.md §Frame, Right region); the options button at its end.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import Icon from "@lucide/svelte/icons/folder-tree";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import ViewTabs from "./ViewTabs.svelte";

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
  component = mount(ViewTabs, { target, props: { views: VIEWS, selected, onselect, onmenu } });
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
    // the name is what the tab shows, the count included (voice control + screen readers)
    expect(live?.getAttribute("aria-label")).toBeNull();
    expect(live?.textContent?.replace(/\s+/g, " ").trim()).toBe("Live changes 3");
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

  it("a tab that takes the focus by itself (a click, a script) keeps the Tab stop while the focus is in the strip", () => {
    const { root } = show("outline");
    tabs(root)[1]?.focus();
    flushSync();
    expect([tabs(root)[0]?.tabIndex, tabs(root)[1]?.tabIndex]).toEqual([-1, 0]);
    key(root, { key: "ArrowRight" });
    expect(document.activeElement).toBe(tabs(root)[0]);
  });
});

describe("the options button of a strip", () => {
  const button = (root: HTMLElement): HTMLButtonElement | null =>
    root.querySelector<HTMLButtonElement>("button[data-view-menu]");

  function showWith(
    selected: string | null,
    more: Record<string, unknown>,
  ): { root: HTMLElement; onmenu: ReturnType<typeof vi.fn> } {
    const target = document.createElement("div");
    document.body.append(target);
    const onmenu = vi.fn();
    component = mount(ViewTabs, {
      target,
      props: { views: VIEWS, selected, onselect: vi.fn(), onmenu, ...more },
    });
    flushSync();
    return { root: target, onmenu };
  }

  it("is absent unless asked for; a right click on a tab opens the options all the same", () => {
    const { root, onmenu } = show("outline");
    expect(button(root)).toBeNull();
    tabs(root)[1]?.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 5, clientY: 6 }),
    );
    expect(onmenu).toHaveBeenCalledWith("live-feed.panel", { x: 5, y: 6 }, tabs(root)[1]);
  });

  it("sits after the tabs, outside their list, named for the selected tab; it opens that tab's options", () => {
    const onoptions = vi.fn();
    const { root } = showWith("live-feed.panel", { onoptions });
    const more = button(root);
    const strip = root.querySelector('[role="tablist"]');
    expect(more?.getAttribute("type")).toBe("button");
    expect(more?.getAttribute("data-view-menu")).toBe("live-feed.panel");
    expect(more?.getAttribute("aria-label")).toBe("Live changes options");
    expect(more?.getAttribute("title")).toBe("Live changes options");
    expect(more?.getAttribute("aria-haspopup")).toBe("menu");
    expect(more?.getAttribute("aria-expanded")).toBe("false");
    // a Tab stop of its own, kept while it is not seen; never among the tabs
    expect(more?.tabIndex).toBe(0);
    expect(more?.hidden).toBe(false);
    expect(strip?.contains(more)).toBe(false);
    expect(strip?.nextElementSibling).toBe(more);
    expect([...(strip?.children ?? [])].map((el) => el.getAttribute("role"))).toEqual([
      "tab",
      "tab",
    ]);
    more?.click();
    expect(onoptions).toHaveBeenCalledWith("live-feed.panel", more);
    // the arrows of the tabs are not its keys
    more?.focus();
    more?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
    expect(document.activeElement).toBe(more);
  });

  it("says when its menu is open; nothing selected: the first tab's options", () => {
    const { root } = showWith(null, { onoptions: vi.fn(), expanded: true });
    expect(button(root)?.getAttribute("aria-expanded")).toBe("true");
    expect(button(root)?.getAttribute("data-view-menu")).toBe("outline");
    expect(button(root)?.getAttribute("aria-label")).toBe("Outline options");
  });

  it("is hidden by opacity alone until the strip is pointed at or holds the focus; always shown where nothing hovers", () => {
    const css = readFileSync(resolve(process.cwd(), "src/core/shell/ViewTabs.svelte"), "utf8");
    const rule = (selector: string): string => {
      const at = css.indexOf(`${selector} {`);
      return at < 0 ? "" : css.slice(at, css.indexOf("}", at));
    };
    expect(rule("  .options")).toMatch(/opacity: 0;/);
    expect(rule("  .options")).not.toMatch(/display|visibility/);
    expect(rule("  .options")).toMatch(/width: 24px;[\s\S]*height: 24px;/);
    expect(css).toMatch(
      /\.bar:hover \.options,\s*\.bar:focus-within \.options,\s*\.options\[aria-expanded="true"\] \{\s*opacity: 1;/,
    );
    expect(css).toMatch(/@media \(hover: none\) \{\s*\.options \{\s*opacity: 1;/);
  });
});

describe("a tab as the handle of its view", () => {
  it("reports a press with the pointer, and shows which tab is being dragged", () => {
    const target = document.createElement("div");
    document.body.append(target);
    const ongrab = vi.fn();
    component = mount(ViewTabs, {
      target,
      props: { views: VIEWS, selected: "outline", onselect: vi.fn(), ongrab, dragging: "outline" },
    });
    flushSync();
    const [outline, live] = tabs(target);
    const press = new PointerEvent("pointerdown", { bubbles: true, button: 0, pointerId: 1 });
    live?.querySelector(".label")?.dispatchEvent(press);
    expect(ongrab).toHaveBeenCalledTimes(1);
    expect(ongrab).toHaveBeenCalledWith("live-feed.panel", live, press);
    expect(outline?.classList.contains("dragged")).toBe(true);
    expect(live?.classList.contains("dragged")).toBe(false);
    // a pen | a finger on it never scrolls the page away from under a drag
    const css = readFileSync(resolve(process.cwd(), "src/core/shell/ViewTabs.svelte"), "utf8");
    expect(css).toMatch(/^ {2}\.tab \{[^}]*touch-action: none;/m);
    expect(css).toMatch(/\.tab\.dragged \{\s*opacity: 0\.\d+;/);
  });
});

describe("a strip too narrow for its labels", () => {
  const css = readFileSync(resolve(process.cwd(), "src/core/shell/ViewTabs.svelte"), "utf8");
  /** The declarations of the rule that starts a line with `selector`. */
  const rule = (selector: string): string => {
    const at = css.split("\n").findIndex((line) => line === `  ${selector} {`);
    return at < 0 ? "" : (css.split("\n").slice(at).join("\n").split("}")[0] ?? "");
  };

  it("the tab shown keeps its whole label: every other tab gives way before it starts to", () => {
    // room is taken from the tabs by these weights: the one shown loses nothing while another still can
    expect(rule(".tab")).toMatch(/flex: 0 100000 auto;/);
    expect(rule('.tab[aria-selected="true"]')).toMatch(/flex-shrink: 1;/);
    expect(rule('.tab[aria-selected="true"]')).not.toMatch(/min-width|max-width/);
  });

  it("another tab shortens its label down to a few letters, then shows its icon alone; without an icon the letters stay", () => {
    const target = document.createElement("div");
    document.body.append(target);
    component = mount(ViewTabs, {
      target,
      props: {
        views: [
          { id: "a", label: "Files", icon: Icon, badge: null },
          { id: "b", label: "Broken links", icon: Icon, badge: "2" },
          { id: "c", label: "Plain", icon: undefined, badge: null },
        ],
        selected: "a",
        onselect: vi.fn(),
        roomy: 3,
      },
    });
    flushSync();
    const [a, b, c] = tabs(target);
    expect([a, b, c].map((tab) => tab?.classList.contains("has-icon"))).toEqual([
      true,
      true,
      false,
    ]);
    expect([a, b, c].map((tab) => tab?.classList.contains("has-badge"))).toEqual([
      false,
      true,
      false,
    ]);
    // three tabs at the left are not crowded: every label is in the page (the strip's width decides what shows)
    expect([a, b, c].map((tab) => tab?.querySelector(".label")?.textContent)).toEqual([
      "Files",
      "Broken links",
      "Plain",
    ]);
    // with an icon: never narrower than the icon; the label is whole-or-ellipsis above the floor, gone under it
    const withIcon = rule('.tab.has-icon:not([aria-selected="true"])');
    expect(withIcon).toMatch(/min-width: calc\(14px \+ 2 \* var\(--tab-pad\)\);/);
    expect(rule('.tab.has-icon:not([aria-selected="true"]) .label')).toMatch(
      /max-width: max\(0px, calc\(\(100% - var\(--label-floor\)\) \* 999\)\);/,
    );
    // without one: the label keeps a readable width
    expect(rule('.tab:not(.has-icon):not([aria-selected="true"]) .label')).toMatch(
      /min-width: 3em;/,
    );
  });

  it("past its roomy count the strip still shows the icons alone of the tabs that are not shown", () => {
    const target = document.createElement("div");
    document.body.append(target);
    component = mount(ViewTabs, {
      target,
      props: {
        views: [
          { id: "a", label: "Files", icon: Icon, badge: null },
          { id: "b", label: "Search", icon: Icon, badge: null },
          { id: "c", label: "Tags", icon: Icon, badge: null },
        ],
        selected: "b",
        onselect: vi.fn(),
        roomy: 2,
      },
    });
    flushSync();
    expect(tabs(target).map((tab) => tab.classList.contains("icon-only"))).toEqual([
      true,
      false,
      true,
    ]);
    expect(tabs(target).map((tab) => tab.querySelector(".label")?.textContent ?? null)).toEqual([
      null,
      "Search",
      null,
    ]);
  });
});
