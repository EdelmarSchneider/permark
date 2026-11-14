// @vitest-environment jsdom
// The favorites menu of the symbols panel (a right click | Shift+F10 | the Menu key on a cell or a group header) and
// the star on each group header.
import "../../core/testing/media";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CharacterGroup } from "../../api";
import { windowStore } from "../../core/window.svelte";
import { symbols } from "./store.svelte";
import SymbolsPanel from "./SymbolsPanel.svelte";

const GROUPS: CharacterGroup[] = [
  { name: "greek", entries: [{ char: "α", names: ["alpha"] }] },
  { name: "arrows", entries: [{ char: "→", names: ["rightwards arrow"] }] },
];

let component: Record<string, unknown> | null = null;
let root: HTMLElement;

beforeEach(() => {
  vi.spyOn(windowStore, "refreshCharacters").mockResolvedValue(undefined);
  windowStore.characters = GROUPS;
  root = document.createElement("div");
  document.body.append(root);
  component = mount(SymbolsPanel, { target: root, props: {} });
  flushSync();
});

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
  windowStore.characters = [];
  windowStore.symbolFavorites = null;
  vi.restoreAllMocks();
});

const cell = (name: string): HTMLElement => {
  const found = root.querySelector<HTMLElement>(`[data-kind="cell"][aria-label="${name}"]`);
  if (found === null) throw new Error(`no cell ${name}`);
  return found;
};
const header = (section: string): HTMLElement => {
  const found = root.querySelector<HTMLElement>(`[data-kind="header"][data-section="${section}"]`);
  if (found === null) throw new Error(`no header ${section}`);
  return found;
};
const items = (): string[] =>
  [...document.querySelectorAll('[role="menu"] [role="menuitem"]')].map(
    (item) => item.textContent?.trim() ?? "",
  );

function rightClick(el: HTMLElement): MouseEvent {
  const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
  el.dispatchEvent(event);
  flushSync();
  return event;
}

function press(el: HTMLElement, init: KeyboardEventInit): KeyboardEvent {
  const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
  el.dispatchEvent(event);
  flushSync();
  return event;
}

describe("the favorites menu", () => {
  it("a right click on a cell offers the character and its group", () => {
    const event = rightClick(cell("alpha"));
    expect(event.defaultPrevented).toBe(true);
    expect(items()).toEqual(["Add to favorites", "Add the group Greek to favorites"]);
  });

  it("Shift+F10 | the Menu key on a cell open the same menu; its items toggle", () => {
    const toggleChar = vi.spyOn(symbols, "toggleChar").mockResolvedValue();
    const toggleGroup = vi.spyOn(symbols, "toggleGroup").mockResolvedValue();
    const alpha = cell("alpha");
    alpha.focus();
    expect(press(alpha, { key: "F10", shiftKey: true }).defaultPrevented).toBe(true);
    expect(items()).toHaveLength(2);
    document.querySelector<HTMLElement>('[role="menuitem"]')?.click();
    flushSync();
    expect(toggleChar).toHaveBeenCalledWith({ char: "α", names: ["alpha"] });
    expect(document.querySelector('[role="menu"]')).toBeNull();
    // the keyboard goes back to the cell it came from
    expect(document.activeElement).toBe(alpha);
    press(alpha, { key: "ContextMenu" });
    const group = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')][1];
    group?.click();
    flushSync();
    expect(toggleGroup).toHaveBeenCalledWith("greek");
  });

  it("names what is already a favorite as a removal", () => {
    windowStore.symbolFavorites = { v: 1, chars: ["α"], groups: ["greek"] };
    flushSync();
    // the favorite cell on top: only the character; the pinned group's cell: both removals
    const [favorite, pinned] = root.querySelectorAll<HTMLElement>('[aria-label="alpha"]');
    if (favorite === undefined || pinned === undefined) throw new Error("cells missing");
    rightClick(favorite);
    expect(items()).toEqual(["Remove from favorites"]);
    document
      .querySelector('[role="menu"]')
      ?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    flushSync();
    rightClick(pinned);
    expect(items()).toEqual(["Remove from favorites", "Remove the group Greek from favorites"]);
  });

  it("a group header offers the group only", () => {
    rightClick(header("arrows"));
    expect(items()).toEqual(["Add the group Arrows to favorites"]);
  });

  it("elsewhere in the panel the keys stay the view's", () => {
    const search = root.querySelector<HTMLElement>('input[type="search"]');
    if (search === null) throw new Error("no search");
    expect(press(search, { key: "F10", shiftKey: true }).defaultPrevented).toBe(false);
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });
});

describe("the star on a group header", () => {
  it("pins | unpins the group and says its state", () => {
    const toggleGroup = vi.spyOn(symbols, "toggleGroup").mockResolvedValue();
    const star = root.querySelector<HTMLButtonElement>(
      '[aria-label="Add the group Greek to favorites"]',
    );
    expect(star?.getAttribute("aria-pressed")).toBe("false");
    expect(star?.tabIndex).toBe(-1);
    star?.click();
    expect(toggleGroup).toHaveBeenCalledWith("greek");
    windowStore.symbolFavorites = { v: 1, chars: [], groups: ["greek"] };
    flushSync();
    const pinned = root.querySelector('[aria-label="Remove the group Greek from favorites"]');
    expect(pinned?.getAttribute("aria-pressed")).toBe("true");
    // the favorites section has no star
    expect(root.querySelectorAll("[aria-pressed]")).toHaveLength(2);
  });
});
