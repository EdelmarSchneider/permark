// @vitest-environment jsdom
// "Move to a project": the list of recent folders scrolls inside the dialog (never under its buttons), one Tab stop,
// arrows | Home | End move the choice and keep the focused row in view.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { RecentFolder } from "../../api";
import MoveDialog from "./MoveDialog.svelte";
import { nextRow } from "./model";
import type { MoveState, NotesStore } from "./store.svelte";

// jsdom has no layout (and no scrolling): the tests watch the call
Element.prototype.scrollIntoView = () => {};

const recents: RecentFolder[] = Array.from({ length: 20 }, (_, i) => ({
  path: `C:/work/project-${i + 1}`,
  name: `project-${i + 1}`,
  openedMs: 1_000 - i,
  missing: false,
}));

/** The parts of the notes store the dialog uses. */
class FakeStore {
  move = $state<MoveState>({
    status: "open",
    path: "note.md",
    folder: recents[0]?.path ?? null,
    name: "note.md",
    recents,
    error: null,
    busy: false,
  });
  closed = 0;
  chooseMoveFolder(folder: string | null): Promise<void> {
    if (this.move.status === "open" && folder !== null) this.move = { ...this.move, folder };
    return Promise.resolve();
  }
  browseMoveFolder(): Promise<void> {
    return Promise.resolve();
  }
  setMoveName(name: string): void {
    if (this.move.status === "open") this.move = { ...this.move, name };
  }
  closeMove(): void {
    this.closed++;
  }
  confirmMove(): Promise<void> {
    return Promise.resolve();
  }
}

let app: Record<string, unknown> | null = null;
afterEach(() => {
  if (app) unmount(app);
  app = null;
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

function open(): { store: FakeStore; dialog: HTMLElement; list: HTMLElement } {
  const store = new FakeStore();
  const target = document.createElement("div");
  document.body.append(target);
  app = mount(MoveDialog, { target, props: { store: store as unknown as NotesStore } });
  flushSync();
  const dialog = document.querySelector<HTMLElement>("[role='dialog']");
  const list = document.querySelector<HTMLElement>("[role='radiogroup']");
  if (!dialog || !list) throw new Error("no dialog");
  return { store, dialog, list };
}

function rows(list: HTMLElement): HTMLElement[] {
  return Array.from(list.querySelectorAll<HTMLElement>("[role='radio']"));
}

function press(el: Element, key: string): void {
  el.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
  flushSync();
}

describe("nextRow", () => {
  it("moves down | up with wrap, Home | End to the ends, other keys stay", () => {
    expect(nextRow(0, 3, "ArrowDown")).toBe(1);
    expect(nextRow(2, 3, "ArrowDown")).toBe(0);
    expect(nextRow(-1, 3, "ArrowRight")).toBe(0);
    expect(nextRow(0, 3, "ArrowUp")).toBe(2);
    expect(nextRow(-1, 3, "ArrowLeft")).toBe(2);
    expect(nextRow(1, 3, "Home")).toBe(0);
    expect(nextRow(1, 3, "End")).toBe(2);
    expect(nextRow(1, 3, "x")).toBe(1);
  });
});

describe("move to a project: the folder list", () => {
  it("is its own scroller inside the dialog body; the name field and the buttons are outside it", () => {
    const { dialog, list } = open();
    expect(list.classList.contains("list")).toBe(true);
    expect(rows(list)).toHaveLength(20);
    const body = dialog.querySelector(".body");
    expect(body?.contains(list)).toBe(true);
    const footer = dialog.querySelector("footer");
    expect(footer?.querySelectorAll("button")).toHaveLength(2);
    expect(list.contains(dialog.querySelector("input"))).toBe(false);
    expect(list.contains(dialog.querySelector(".browse"))).toBe(false);
    // the list is labelled by the visible "Project folder" label
    const label = document.getElementById(list.getAttribute("aria-labelledby") ?? "");
    expect(label?.textContent?.trim()).not.toBe("");
  });

  it("is one Tab stop: the chosen row; Tab wraps inside the dialog over it", async () => {
    const { store, dialog, list } = open();
    await store.chooseMoveFolder("C:/work/project-5");
    flushSync();
    const all = rows(list);
    const stops = all.filter((r) => r.tabIndex === 0);
    expect(stops).toEqual([all[4]]);
    expect(all[4]?.getAttribute("aria-checked")).toBe("true");
    const buttons = Array.from(dialog.querySelectorAll<HTMLButtonElement>("footer button"));
    const move = buttons.at(-1);
    // Shift+Tab on the chosen row (the first Tab stop) goes round to Move, Tab on Move back to it
    all[4]?.focus();
    all[4]?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Tab", shiftKey: true, bubbles: true, cancelable: true }),
    );
    expect(document.activeElement).toBe(move);
    move?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true }),
    );
    expect(document.activeElement).toBe(all[4]);
  });

  it("arrows | Home | End move the choice, the focus and keep the row in view", () => {
    const seen: Element[] = [];
    vi.spyOn(Element.prototype, "scrollIntoView").mockImplementation(function (this: Element) {
      seen.push(this);
    });
    const { store, list } = open();
    const all = rows(list);
    all[0]?.focus();
    press(all[0] ?? list, "ArrowDown");
    expect(document.activeElement).toBe(all[1]);
    expect(store.move.status === "open" && store.move.folder).toBe("C:/work/project-2");
    expect(all[1]?.getAttribute("aria-checked")).toBe("true");
    expect(all[1]?.tabIndex).toBe(0);
    expect(all[0]?.tabIndex).toBe(-1);
    press(all[1] ?? list, "End");
    expect(document.activeElement).toBe(all[19]);
    expect(store.move.status === "open" && store.move.folder).toBe("C:/work/project-20");
    press(all[19] ?? list, "ArrowDown");
    expect(document.activeElement).toBe(all[0]);
    press(all[0] ?? list, "ArrowUp");
    expect(document.activeElement).toBe(all[19]);
    press(all[19] ?? list, "Home");
    expect(document.activeElement).toBe(all[0]);
    expect(seen).toEqual([all[1], all[19], all[0], all[19], all[0]]);
  });

  it("a key with Ctrl | Alt, or another key, does nothing to the choice", () => {
    const { store, list } = open();
    const all = rows(list);
    all[0]?.focus();
    all[0]?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowDown", ctrlKey: true, bubbles: true }),
    );
    press(all[0] ?? list, "a");
    flushSync();
    expect(document.activeElement).toBe(all[0]);
    expect(store.move.status === "open" && store.move.folder).toBe("C:/work/project-1");
  });

  it("no recent folder: the hint, no list", () => {
    const store = new FakeStore();
    store.move = { ...store.move, recents: [], folder: null } as MoveState;
    const target = document.createElement("div");
    document.body.append(target);
    app = mount(MoveDialog, { target, props: { store: store as unknown as NotesStore } });
    flushSync();
    expect(document.querySelector("[role='radiogroup']")).toBeNull();
    expect(document.querySelector(".faint")).not.toBeNull();
  });
});
