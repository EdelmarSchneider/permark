// @vitest-environment jsdom
// The symbols panel: insert at the caret (a click keeps the editor's keyboard, Enter keeps the panel's), the note
// while nothing can take a character, the keys of the list, the favorites menu (a right click | Shift+F10 | the
// Menu key on a cell or a group header) and the star on each group header, and where the keyboard lands when a
// favorite change takes away what had it.
import "../../core/testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CharacterGroup, SymbolFavorites } from "../../api";
import { documents } from "../../core/doc/document.svelte";
import { editing } from "../../core/editing/session.svelte";
import { arrangement } from "../../core/shell/arrangement.svelte";
import { windowStore } from "../../core/window.svelte";
import { symbols } from "./store.svelte";
import SymbolsPanel from "./SymbolsPanel.svelte";

const GROUPS: CharacterGroup[] = [
  {
    name: "greek",
    entries: [
      { char: "α", names: ["alpha"] },
      { char: "β", names: ["beta"] },
      { char: "γ", names: ["gamma"] },
      { char: "δ", names: ["delta"] },
    ],
  },
  {
    name: "arrows",
    entries: [
      { char: "→", names: ["rightwards arrow"] },
      { char: "←", names: ["leftwards arrow"] },
    ],
  },
];

let component: Record<string, unknown> | null = null;
let root: HTMLElement;
let refresh: ReturnType<typeof vi.spyOn>;

/** The editor the panel sees: Edit | Source on the shown document, View on it, or no document. */
function editor(mode: "edit" | "source" | "view" | "none"): void {
  vi.spyOn(documents, "path", "get").mockReturnValue(mode === "none" ? null : "a.md");
  vi.spyOn(editing, "path", "get").mockReturnValue(mode === "none" ? null : "a.md");
  vi.spyOn(editing, "mode", "get").mockReturnValue(mode === "none" ? "view" : mode);
}

function open(favorites: SymbolFavorites | null = null): void {
  windowStore.symbolFavorites = favorites;
  component = mount(SymbolsPanel, { target: root, props: {} });
  flushSync();
}

beforeEach(() => {
  refresh = vi.spyOn(windowStore, "refreshCharacters").mockImplementation(() => {});
  vi.spyOn(arrangement, "say").mockImplementation(() => {});
  windowStore.characters = GROUPS;
  root = document.createElement("div");
  document.body.append(root);
});

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
  windowStore.characters = [];
  windowStore.symbolFavorites = null;
  symbols.reset();
  vi.restoreAllMocks();
});

const cell = (name: string, section?: string): HTMLElement => {
  const where = section === undefined ? "" : `[data-section="${section}"]`;
  const found = root.querySelector<HTMLElement>(`[data-kind="cell"]${where}[aria-label="${name}"]`);
  if (found === null) throw new Error(`no cell ${name}`);
  return found;
};
const header = (section: string): HTMLElement => {
  const found = root.querySelector<HTMLElement>(`[data-kind="header"][data-section="${section}"]`);
  if (found === null) throw new Error(`no header ${section}`);
  return found;
};
const searchField = (): HTMLInputElement => {
  const found = root.querySelector<HTMLInputElement>('input[type="search"]');
  if (found === null) throw new Error("no search");
  return found;
};
const items = (): string[] =>
  [...document.querySelectorAll('[role="menu"] [role="menuitem"]')].map(
    (item) => item.textContent?.trim() ?? "",
  );
const menuItem = (label: string): HTMLElement => {
  const found = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find(
    (item) => item.textContent?.trim() === label,
  );
  if (found === undefined) throw new Error(`no item ${label}`);
  return found;
};

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

/** Lets the awaited steps (a favorite write, a focus move after a redraw) finish. */
async function settle(): Promise<void> {
  for (let n = 0; n < 4; n++) {
    await tick();
    flushSync();
  }
}

/** A grid of three columns, 32 px cells, each group's rows below its own offset: what Up | Down measure. */
function drawGrid(): void {
  const top: Record<string, number> = { greek: 100, arrows: 300 };
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
    this: HTMLElement,
  ) {
    const stop = this.dataset.stop ?? "";
    const index = Number(stop.slice(stop.lastIndexOf("\u0001") + 1));
    const section = this.dataset.section ?? "";
    const left = Number.isNaN(index) ? 0 : (index % 3) * 32;
    const y = (top[section] ?? 0) + (Number.isNaN(index) ? -32 : Math.floor(index / 3) * 32);
    return DOMRect.fromRect({ x: left, y, width: 32, height: 32 });
  });
}

describe("putting a character in", () => {
  it("reads the groups file again once when it opens", () => {
    editor("edit");
    open();
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("a click inserts at the caret; the press never takes the editor's keyboard", () => {
    editor("edit");
    const insert = vi.spyOn(editing, "insert").mockReturnValue(true);
    open();
    const alpha = cell("alpha");
    const down = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
    alpha.dispatchEvent(down);
    expect(down.defaultPrevented).toBe(true);
    alpha.click();
    expect(insert).toHaveBeenCalledWith("α");
    expect(arrangement.say).toHaveBeenLastCalledWith("Inserted alpha");
  });

  it("Enter on a cell inserts and the keyboard stays on the cell", () => {
    editor("source");
    const insert = vi.spyOn(editing, "insert").mockReturnValue(true);
    open();
    const beta = cell("beta");
    beta.focus();
    // the list leaves Enter to the button, whose click (the browser's own) puts the character in
    expect(press(beta, { key: "Enter" }).defaultPrevented).toBe(false);
    beta.click();
    flushSync();
    expect(insert).toHaveBeenCalledWith("β");
    expect(document.activeElement).toBe(beta);
  });

  it("in View the cells are unavailable, say why, and a click inserts nothing", () => {
    editor("view");
    const insert = vi.spyOn(editing, "insert").mockReturnValue(true);
    open();
    const note = root.querySelector<HTMLElement>(".note");
    expect(note?.textContent?.trim()).toBe("Switch to Edit to insert symbols.");
    const alpha = cell("alpha");
    expect(alpha.getAttribute("aria-disabled")).toBe("true");
    expect(alpha.getAttribute("aria-describedby")).toBe(note?.id);
    alpha.click();
    expect(insert).not.toHaveBeenCalled();
  });

  it("with no document the note says to open one", () => {
    editor("none");
    open();
    expect(root.querySelector(".note")?.textContent?.trim()).toBe(
      "Open a document to insert symbols.",
    );
    expect(cell("alpha").getAttribute("aria-disabled")).toBe("true");
  });

  it("with an editor there is no note and the cells are available", () => {
    editor("edit");
    open();
    expect(root.querySelector(".note")).toBeNull();
    expect(cell("alpha").hasAttribute("aria-disabled")).toBe(false);
    expect(cell("alpha").hasAttribute("aria-describedby")).toBe(false);
  });
});

describe("the keys of the list", () => {
  beforeEach(() => {
    editor("edit");
    drawGrid();
    open();
  });

  const at = (): string | undefined =>
    document.activeElement instanceof HTMLElement
      ? `${document.activeElement.dataset.section}:${document.activeElement.getAttribute("aria-label") ?? "header"}`
      : undefined;

  async function key(init: KeyboardEventInit): Promise<void> {
    const now = document.activeElement;
    if (!(now instanceof HTMLElement)) throw new Error("nothing focused");
    press(now, init);
    await settle();
  }

  it("Right | Left go across a group boundary; on a header they expand | collapse it", async () => {
    cell("delta").focus();
    await key({ key: "ArrowRight" });
    expect(at()).toBe("arrows:header");
    await key({ key: "ArrowLeft" });
    expect(header("arrows").getAttribute("aria-expanded")).toBe("false");
    expect(at()).toBe("arrows:header");
    await key({ key: "ArrowLeft" });
    expect(at()).toBe("greek:delta");
    await key({ key: "ArrowRight" });
    await key({ key: "ArrowRight" });
    expect(header("arrows").getAttribute("aria-expanded")).toBe("true");
    await key({ key: "ArrowRight" });
    expect(at()).toBe("arrows:rightwards arrow");
  });

  it("Down | Up go to the nearest cell of the next | previous row, past the rows to a header", async () => {
    cell("beta").focus();
    await key({ key: "ArrowDown" });
    expect(at()).toBe("greek:delta");
    await key({ key: "ArrowDown" });
    expect(at()).toBe("arrows:header");
    cell("delta").focus();
    await key({ key: "ArrowUp" });
    expect(at()).toBe("greek:alpha");
    await key({ key: "ArrowUp" });
    expect(at()).toBe("greek:header");
  });

  it("Page Down goes to the next group's header", async () => {
    cell("alpha").focus();
    await key({ key: "PageDown" });
    expect(at()).toBe("arrows:header");
  });

  it("a letter goes to the search with it; Escape there clears, then gives the keyboard to the editor", async () => {
    const page = document.body.appendChild(document.createElement("div"));
    page.dataset.region = "page";
    const text = page.appendChild(document.createElement("div"));
    text.className = "cm-content";
    text.tabIndex = -1;
    cell("alpha").focus();
    await key({ key: "x" });
    expect(document.activeElement).toBe(searchField());
    expect(symbols.query).toBe("x");
    await key({ key: "Escape" });
    expect(symbols.query).toBe("");
    expect(document.activeElement).toBe(searchField());
    await key({ key: "Escape" });
    expect(document.activeElement).toBe(text);
  });
});

describe("the favorites menu", () => {
  beforeEach(() => editor("edit"));

  it("a right click on a cell offers the character and its group, named for the character", () => {
    open();
    const event = rightClick(cell("alpha"));
    expect(event.defaultPrevented).toBe(true);
    expect(items()).toEqual(["Add to favorites", "Add the group Greek to favorites"]);
    expect(document.querySelector('[role="menu"]')?.getAttribute("aria-label")).toBe(
      "Options for alpha",
    );
  });

  it("Shift+F10 | the Menu key on a cell open the same menu; its items toggle", async () => {
    const toggleChar = vi.spyOn(symbols, "toggleChar").mockResolvedValue();
    const toggleGroup = vi.spyOn(symbols, "toggleGroup").mockResolvedValue();
    open();
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
    await settle();
    press(alpha, { key: "ContextMenu" });
    menuItem("Add the group Greek to favorites").click();
    flushSync();
    expect(toggleGroup).toHaveBeenCalledWith("greek");
  });

  it("names what is already a favorite as a removal", () => {
    open({ v: 1, chars: ["α"], groups: ["greek"] });
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

  it("a right click on a group header offers the group only and gives the keyboard to the header", () => {
    open();
    const arrows = header("arrows");
    rightClick(arrows);
    expect(items()).toEqual(["Add the group Arrows to favorites"]);
    expect(document.querySelector('[role="menu"]')?.getAttribute("aria-label")).toBe(
      "Options for Arrows",
    );
    document
      .querySelector('[role="menu"]')
      ?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    flushSync();
    expect(document.activeElement).toBe(arrows);
  });

  it("elsewhere in the panel the keys stay the view's", () => {
    open();
    expect(press(searchField(), { key: "F10", shiftKey: true }).defaultPrevented).toBe(false);
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });
});

describe("the keyboard after a favorite change", () => {
  beforeEach(() => editor("edit"));

  /** The write's `settings-changed` lands at once. */
  function writesLand(): void {
    vi.spyOn(symbols, "toggleChar").mockImplementation(async (entry) => {
      const chars = (windowStore.symbolFavorites?.chars ?? []).filter((c) => c !== entry.char);
      windowStore.symbolFavorites =
        chars.length === 0 && (windowStore.symbolFavorites?.groups.length ?? 0) === 0
          ? null
          : { v: 1, chars, groups: windowStore.symbolFavorites?.groups ?? [] };
    });
    vi.spyOn(symbols, "toggleGroup").mockImplementation(async (name) => {
      const groups = (windowStore.symbolFavorites?.groups ?? []).filter((g) => g !== name);
      windowStore.symbolFavorites = {
        v: 1,
        chars: windowStore.symbolFavorites?.chars ?? [],
        groups,
      };
    });
  }

  it("the last favorite removed: the same character in its own group takes it", async () => {
    writesLand();
    open({ v: 1, chars: ["β"], groups: [] });
    const favorite = root.querySelector<HTMLElement>('[data-kind="cell"][aria-label="beta"]');
    if (favorite === null) throw new Error("no favorite");
    expect(favorite.dataset.section).not.toBe("greek");
    favorite.focus();
    press(favorite, { key: "F10", shiftKey: true });
    menuItem("Remove from favorites").click();
    await settle();
    expect(document.activeElement).toBe(cell("beta", "greek"));
  });

  it("a favorite no group holds removed: the favorites title takes it", async () => {
    writesLand();
    open({ v: 1, chars: ["Ж"], groups: [] });
    const favorite = cell("U+0416");
    favorite.focus();
    press(favorite, { key: "ContextMenu" });
    menuItem("Remove from favorites").click();
    await settle();
    expect(document.activeElement?.hasAttribute("data-favorites-title")).toBe(true);
  });

  it("a pinned group unpinned from its header: the header keeps it where the group goes", async () => {
    writesLand();
    open({ v: 1, chars: [], groups: ["arrows"] });
    const arrows = header("arrows");
    arrows.focus();
    press(arrows, { key: "F10", shiftKey: true });
    menuItem("Remove the group Arrows from favorites").click();
    await settle();
    expect(document.activeElement).toBe(header("arrows"));
    // it went back below the greek group
    const order = [...root.querySelectorAll<HTMLElement>('[data-kind="header"]')].map(
      (h) => h.dataset.section,
    );
    expect(order).toEqual(["greek", "arrows"]);
  });
});

describe("the star on a group header", () => {
  it("pins | unpins the group; one name, its state in aria-pressed", () => {
    editor("edit");
    const toggleGroup = vi.spyOn(symbols, "toggleGroup").mockResolvedValue();
    open();
    const star = root.querySelector<HTMLButtonElement>(
      '[aria-label="Keep the group Greek as a favorite"]',
    );
    expect(star?.getAttribute("aria-pressed")).toBe("false");
    expect(star?.tabIndex).toBe(-1);
    star?.click();
    expect(toggleGroup).toHaveBeenCalledWith("greek");
    windowStore.symbolFavorites = { v: 1, chars: [], groups: ["greek"] };
    flushSync();
    const pinned = root.querySelector('[aria-label="Keep the group Greek as a favorite"]');
    expect(pinned?.getAttribute("aria-pressed")).toBe("true");
    // the favorites section has no star
    expect(root.querySelectorAll("[aria-pressed]")).toHaveLength(2);
  });
});

describe("the header", () => {
  it("puts the view's buttons before the search, so Tab meets them in the order of their lines", () => {
    editor("edit");
    open();
    const header = root.querySelector("header");
    const close = header?.querySelector('[aria-label="Close the symbols panel"]');
    const search = header?.querySelector('input[type="search"]');
    if (!close || !search) throw new Error("header parts missing");
    expect(close.compareDocumentPosition(search) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});

describe("the search", () => {
  it("says how many characters it found once the typing pauses, and when none answers", () => {
    vi.useFakeTimers();
    try {
      editor("edit");
      open();
      const field = searchField();
      const type = (text: string): void => {
        field.value = text;
        field.dispatchEvent(new Event("input", { bubbles: true }));
        flushSync();
      };
      type("a");
      type("arrow");
      expect(arrangement.say).not.toHaveBeenCalled();
      vi.advanceTimersByTime(1000);
      flushSync();
      // one line for the pause, with the count of what is listed now
      expect(arrangement.say).toHaveBeenCalledTimes(1);
      expect(arrangement.say).toHaveBeenLastCalledWith("2 results");
      type("zzzz");
      vi.advanceTimersByTime(1000);
      expect(arrangement.say).toHaveBeenLastCalledWith("No name matches that search");
      // the search emptied: nothing to count, nothing said
      type("");
      vi.advanceTimersByTime(1000);
      expect(arrangement.say).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });
});
