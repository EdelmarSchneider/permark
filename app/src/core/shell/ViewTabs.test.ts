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

/** After the next animation frame, with what it changed drawn. */
const nextFrame = async (): Promise<void> => {
  await new Promise((done) => requestAnimationFrame(done));
  flushSync();
};

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

  it("a tab that gives way, the one shown too, shortens its label down to a few letters, then shows its icon alone; without an icon the letters stay", () => {
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
    // with an icon: never narrower than the icon; the label is whole-or-ellipsis above the floor, gone under it (the
    // tab shown as well: it only gives way last, by its weight)
    const withIcon = rule(".tab.has-icon");
    expect(withIcon).toMatch(/min-width: calc\(14px \+ 2 \* var\(--tab-pad\)\);/);
    expect(rule(".tab.has-icon .label")).toMatch(
      /max-width: max\(0px, calc\(\(100% - var\(--label-floor\)\) \* 999\)\);/,
    );
    // without one: the label keeps a readable width
    expect(rule(".tab:not(.has-icon) .label")).toMatch(/min-width: 3em;/);
  });

  /** A crowded strip of six tabs, the third one shown (`shownIcon`: whether it has an icon), sized by the test:
   *  `size(width)` gives the strip that width (its tabs get it less 16 px of padding and 32 px of options button) and
   *  draws the tab shown 82 px wide with its label cut to 46 of its 101 px. The five other tabs, icons alone, take
   *  26 px each with their gap. */
  function crowdedStrip(shownIcon: boolean): {
    target: HTMLElement;
    onselect: ReturnType<typeof vi.fn>;
    drawn: () => (string | null)[];
    more: () => HTMLButtonElement | null;
    shownTab: () => HTMLElement | null;
    size: (width: number) => void;
  } {
    const observers: (() => void)[] = [];
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(callback: () => void) {
          observers.push(callback);
        }
        observe(): void {}
        disconnect(): void {}
      },
    );
    const target = document.createElement("div");
    document.body.append(target);
    const onselect = vi.fn();
    const views = ["Files", "Search", "Broken links", "Comments", "Tags", "Unread"].map(
      (label, at) => ({
        id: `v${at}`,
        label,
        icon: at === 2 && !shownIcon ? undefined : Icon,
        badge: at === 5 ? "1" : null,
      }),
    );
    component = mount(ViewTabs, {
      target,
      props: {
        views,
        selected: "v2",
        onselect,
        onoptions: vi.fn(),
        label: "Sidebar views",
        roomy: 3,
      },
    });
    flushSync();
    const shownTab = (): HTMLElement | null =>
      target.querySelector<HTMLElement>('[aria-selected="true"]');
    return {
      target,
      onselect,
      drawn: () => tabs(target).map((tab) => tab.dataset["view"] ?? null),
      more: () => target.querySelector<HTMLButtonElement>('button[aria-label="Sidebar views"]'),
      shownTab,
      size: (width) => {
        const bar = target.querySelector<HTMLElement>(".bar");
        const tab = shownTab();
        const label = tab?.querySelector<HTMLElement>(".label");
        if (!bar || !tab || !label) throw new Error("no strip");
        Object.defineProperty(bar, "clientWidth", { configurable: true, get: () => width });
        tab.getBoundingClientRect = () => DOMRect.fromRect({ width: 82, height: 28 });
        label.getBoundingClientRect = () => DOMRect.fromRect({ width: 46, height: 16 });
        Object.defineProperty(label, "scrollWidth", { configurable: true, get: () => 101 });
        for (const observer of observers) observer();
        flushSync();
      },
    };
  }
  const ALL = ["v0", "v1", "v2", "v3", "v4", "v5"];

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("a crowded strip keeps every tab while all fit as icons: the label of the tab shown gives way first, then the last other tabs move into a menu of its views", async () => {
    const { target, onselect, drawn, more, shownTab, size } = crowdedStrip(true);
    // not measured yet: every tab, no menu button
    expect(drawn()).toEqual(ALL);
    expect(more()).toBeNull();
    // wide: the tab shown with its whole label, the others as icons, nothing in a menu
    size(400);
    await nextFrame();
    expect(drawn()).toEqual(ALL);
    expect(more()).toBeNull();
    expect(tabs(target).map((tab) => tab.classList.contains("icon-only"))).toEqual([
      true,
      true,
      false,
      true,
      true,
      true,
    ]);
    expect(shownTab()?.querySelector(".label")?.textContent).toBe("Broken links");
    // room for the icons and a few letters of the label shown: its label shortens (CSS), no tab leaves; the label
    // stays in the page, so the tab keeps its name
    size(244);
    await nextFrame();
    expect(drawn()).toEqual(ALL);
    expect(more()).toBeNull();
    expect(shownTab()?.querySelector(".label")?.textContent).toBe("Broken links");
    expect(shownTab()?.getAttribute("title")).toBe("Broken links");
    // room for the six icons alone (36 px for the tab shown): its label at no width, still every tab
    size(214);
    await nextFrame();
    expect(drawn()).toEqual(ALL);
    expect(more()).toBeNull();
    // less than six icons: the last other tabs move into the views menu, in the next frame, not inside the
    // observer's call (a resize there would loop)
    size(180);
    expect(drawn()).toEqual(ALL);
    await nextFrame();
    expect(drawn()).toEqual(["v0", "v1", "v2"]);
    const button = more();
    expect(button?.title).toBe("Sidebar views");
    expect(button?.getAttribute("aria-haspopup")).toBe("menu");
    // the arrows go over the tabs drawn
    key(target, { key: "ArrowRight" });
    expect(document.activeElement).toBe(tabs(target)[0]);
    // the menu lists every view of the strip, the one shown checked (a state, not a word read aloud), a count beside
    // its view
    button?.click();
    flushSync();
    expect(button?.getAttribute("aria-expanded")).toBe("true");
    const items = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitemradio"]'));
    expect(items.map((item) => item.textContent?.trim())).toEqual([
      "Files",
      "Search",
      "Broken links",
      "Comments",
      "Tags",
      "Unread (1)",
    ]);
    expect(items.map((item) => item.getAttribute("aria-checked"))).toEqual([
      "false",
      "false",
      "true",
      "false",
      "false",
      "false",
    ]);
    items[4]?.click();
    flushSync();
    expect(onselect).toHaveBeenCalledWith("v4");
    expect(document.querySelector('[role="menu"]')).toBeNull();
    // room again: every tab is back and the menu button goes
    size(400);
    await nextFrame();
    expect(drawn()).toEqual(ALL);
    expect(more()).toBeNull();
  });

  it("a crowded strip whose tab shown has no icon keeps a few of its letters: the other tabs move out before they go", async () => {
    const { drawn, more, shownTab, size } = crowdedStrip(false);
    // the label's floor (3em in the page) measured in px: 36 + 39 px for the tab shown, 130 for the other five
    const label = shownTab()?.querySelector<HTMLElement>(".label");
    if (!label) throw new Error("no label");
    label.style.minWidth = "39px";
    size(253);
    await nextFrame();
    expect(drawn()).toEqual(ALL);
    expect(more()).toBeNull();
    size(252);
    await nextFrame();
    expect(drawn()).toEqual(["v0", "v1", "v2", "v3", "v4"]);
    expect(more()).not.toBeNull();
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
