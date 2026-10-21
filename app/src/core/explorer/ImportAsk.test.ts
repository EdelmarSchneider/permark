// @vitest-environment jsdom
import "../testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import type { ConflictChoice, ImportMode, ImportPlan, ImportProgress } from "../../api";
import ImportAsk from "./ImportAsk.svelte";
import ImportProgressCard from "./ImportProgress.svelte";

let target: HTMLElement;
let component: Record<string, unknown> | null = null;

beforeEach(() => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
});

const item = (name: string, patch: object = {}) => ({
  source: `C:/out/${name}`,
  name,
  kind: "file" as const,
  inProject: false,
  conflict: false,
  ...patch,
});

function plan(patch: Partial<ImportPlan> = {}): ImportPlan {
  return { planId: 1, target: "docs", items: [item("a.md")], refused: [], ...patch };
}

type Spies = {
  setConflicts: Mock<(conflicts: ConflictChoice) => void>;
  confirm: Mock<(mode: ImportMode) => void>;
  cancel: Mock<() => void>;
};

function open(p: ImportPlan | null, conflicts: ConflictChoice = "keepBoth", where = "docs"): Spies {
  const spies: Spies = {
    setConflicts: vi.fn<(conflicts: ConflictChoice) => void>(),
    confirm: vi.fn<(mode: ImportMode) => void>(),
    cancel: vi.fn<() => void>(),
  };
  component = mount(ImportAsk, {
    target,
    props: { at: { x: 100, y: 120 }, plan: p, where, conflicts, ...spies },
  });
  flushSync();
  return spies;
}

const text = (): string => target.textContent ?? "";
const buttons = (): HTMLButtonElement[] => [...target.querySelectorAll("button")];
const named = (name: string): HTMLButtonElement | undefined =>
  buttons().find((b) => b.textContent?.trim() === name);
const dialog = (): HTMLElement => target.querySelector("[role=dialog]") as HTMLElement;

function key(name: string, init: KeyboardEventInit = {}): void {
  (document.activeElement ?? dialog()).dispatchEvent(
    new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true, ...init }),
  );
  flushSync();
}

describe("the question at the drop point", () => {
  it("names where, what comes in, and the three answers; not modal", () => {
    open(plan({ items: [item("a.md"), item("b.md"), item("pics", { kind: "folder" })] }));
    expect(target.querySelector("h2")?.textContent).toBe("Add to “docs”");
    expect(text()).toContain("2 files and 1 folder");
    expect(dialog().hasAttribute("aria-modal")).toBe(false);
    expect(dialog().getAttribute("aria-labelledby")).toBe("import-ask-title");
    expect(
      document.getElementById(dialog().getAttribute("aria-describedby") ?? "")?.textContent,
    ).toBe("2 files and 1 folder");
    expect(buttons().map((b) => b.textContent?.trim())).toEqual([
      "Copy here",
      "Move here",
      "Cancel",
    ]);
    expect(text()).toContain("Copy keeps the originals where they are.");
    expect(text()).not.toContain("already exist");
  });

  it("Copy here takes the keyboard; Enter copies, never moves", async () => {
    const spies = open(plan());
    await tick();
    flushSync();
    expect(document.activeElement).toBe(named("Copy here"));
    named("Copy here")?.click();
    expect(spies.confirm).toHaveBeenCalledWith("copy");
    named("Move here")?.click();
    expect(spies.confirm).toHaveBeenLastCalledWith("move");
  });

  it("lists the first 4 taken names, then how many more; Keep both | Skip with a hint", () => {
    const items = ["a.md", "b.md", "c.md", "d.md", "e.md", "f.md"].map((n) =>
      item(n, { conflict: true }),
    );
    const spies = open(
      plan({ items: [...items, item("notes", { kind: "folder", conflict: true })] }),
    );
    expect(text()).toContain("7 names already exist here:");
    expect(text()).toContain("a.md, b.md, c.md, d.md and 3 more");
    const group = target.querySelector("[role=radiogroup]");
    const label = document.getElementById(group?.getAttribute("aria-labelledby") ?? "");
    expect(label?.textContent).toBe("If a name already exists:");
    expect(
      [...(group?.querySelectorAll("[role=radio]") ?? [])].map((r) =>
        r.getAttribute("aria-checked"),
      ),
    ).toEqual(["true", "false"]);
    expect(text()).toContain("The new one gets a number, like name (2).");
    named("Skip")?.click();
    expect(spies.setConflicts).toHaveBeenCalledWith("skip");
  });

  it("the Skip hint says nothing is replaced", () => {
    open(plan({ items: [item("a.md", { conflict: true })] }), "skip");
    expect(text()).toContain("Nothing is replaced.");
  });

  it("items already in the project: no Move here, and why", () => {
    open(plan({ items: [item("a.md", { inProject: true }), item("b.md")] }));
    expect(named("Move here")).toBeUndefined();
    expect(text()).toContain("To move them, drag them inside the file list.");
  });

  it("lists what cannot come in, muted, first 3 with their reason", () => {
    const refused = ["x", "y", "z", "w"].map((name) => ({
      source: `C:/out/${name}`,
      name,
      reason: "link" as const,
    }));
    open(plan({ refused }));
    expect(text()).toContain("4 items can't be added:");
    expect(text()).toContain("x is a link, and links aren't followed");
    expect(text()).not.toContain("w is a link");
    expect(text()).toContain("and 1 more");
  });

  it("nothing can come in: says so, the reasons, and Close only", () => {
    const spies = open(
      plan({ items: [], refused: [{ source: "C:/out/.git", name: ".git", reason: "protected" }] }),
    );
    expect(text()).toContain("Nothing here can be added.");
    expect(text()).toContain(".git is reserved for git or PerMark");
    expect(buttons().map((b) => b.textContent?.trim())).toEqual(["Close"]);
    named("Close")?.click();
    expect(spies.cancel).toHaveBeenCalled();
  });

  it("while the plan is on its way: checking names, buttons off except Cancel", () => {
    open(null);
    expect(text()).toContain("Checking names…");
    expect(named("Copy here")?.disabled).toBe(true);
    expect(named("Move here")?.disabled).toBe(true);
    expect(named("Cancel")?.disabled).toBe(false);
  });

  it("Esc and a press outside close it; a press inside does not", () => {
    const spies = open(plan());
    key("Escape");
    expect(spies.cancel).toHaveBeenCalledTimes(1);
    dialog().dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    expect(spies.cancel).toHaveBeenCalledTimes(1);
    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    expect(spies.cancel).toHaveBeenCalledTimes(2);
  });

  it("a press on another tab's chip is a tab switch, not an answer; on the current chip it closes", () => {
    const strip = document.createElement("div");
    strip.innerHTML =
      '<button data-tab aria-current="page"><span>this</span></button><button data-tab><span>other</span></button>';
    document.body.append(strip);
    const spies = open(plan());
    const [current, other] = [...strip.querySelectorAll("span")];
    other?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    expect(spies.cancel).not.toHaveBeenCalled();
    current?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    expect(spies.cancel).toHaveBeenCalledTimes(1);
  });

  it("Ctrl | Cmd | Alt + Tab are never kept inside (they switch tabs | windows)", async () => {
    open(plan());
    await tick();
    flushSync();
    named("Copy here")?.focus();
    for (const init of [{ ctrlKey: true }, { metaKey: true }, { altKey: true }]) {
      const event = new KeyboardEvent("keydown", {
        key: "Tab",
        shiftKey: true,
        bubbles: true,
        cancelable: true,
        ...init,
      });
      document.activeElement?.dispatchEvent(event);
      expect(event.defaultPrevented).toBe(false);
      expect(document.activeElement).toBe(named("Copy here"));
    }
  });

  it("Tab and Shift+Tab stay inside", async () => {
    open(plan());
    await tick();
    flushSync();
    named("Cancel")?.focus();
    key("Tab");
    expect(document.activeElement).toBe(named("Copy here"));
    key("Tab", { shiftKey: true });
    expect(document.activeElement).toBe(named("Cancel"));
  });

  it("gives the keyboard back where it was when it closes", async () => {
    const before = document.createElement("button");
    document.body.append(before);
    before.focus();
    open(plan());
    await tick();
    flushSync();
    expect(document.activeElement).not.toBe(before);
    if (component) await unmount(component);
    component = null;
    expect(document.activeElement).toBe(before);
  });
});

describe("the progress card", () => {
  const progress = (patch: Partial<ImportProgress> = {}): ImportProgress => ({
    planId: 1,
    phase: "copying",
    itemsDone: 1,
    itemsTotal: 4,
    filesDone: 120,
    filesTotal: 3400,
    bytesDone: 250,
    bytesTotal: 1000,
    item: "hundredk",
    ...patch,
  });

  function card(p: ImportProgress | null, cancelling = false, mode: ImportMode = "copy") {
    const stop = vi.fn<() => void>();
    component = mount(ImportProgressCard, {
      target,
      props: { where: "docs", mode, progress: p, cancelling, stop },
    });
    flushSync();
    return stop;
  }

  it("says what it does, where, the item, the counts and a bar", () => {
    card(progress());
    expect(text()).toContain("Copying to “docs”…");
    expect(text()).toContain("hundredk");
    expect(text()).toContain("120 of 3400 files");
    const bar = target.querySelector("[role=progressbar]");
    expect(bar?.getAttribute("aria-valuenow")).toBe("25");
  });

  it("says moving for a move, and looking at the files while it counts", () => {
    card(progress(), false, "move");
    expect(text()).toContain("Moving to “docs”…");
    unmountNow();
    card(progress({ phase: "counting" }));
    expect(text()).toContain("Looking at the files…");
    expect(target.querySelector("[role=progressbar]")?.hasAttribute("aria-valuenow")).toBe(false);
  });

  it("before the first report there is a bar with no value and no counts", () => {
    card(null);
    expect(text()).toContain("Looking at the files…");
    expect(target.querySelector(".indeterminate")).not.toBeNull();
  });

  it("counts items when no file total is known", () => {
    card(progress({ filesTotal: 0, filesDone: 0, bytesTotal: 0, bytesDone: 0 }));
    expect(text()).toContain("1 of 4 items");
    expect(target.querySelector("[role=progressbar]")?.getAttribute("aria-valuenow")).toBe("25");
  });

  it("Cancel asks to stop once and then shows it is stopping", () => {
    const stop = card(progress());
    named("Cancel")?.click();
    expect(stop).toHaveBeenCalledTimes(1);
    unmountNow();
    card(progress(), true);
    expect(text()).toContain("Stopping…");
    expect(named("Cancel")?.disabled).toBe(true);
  });

  it("never reports more than all", () => {
    card(progress({ bytesDone: 5000, bytesTotal: 1000 }));
    expect(target.querySelector("[role=progressbar]")?.getAttribute("aria-valuenow")).toBe("100");
  });
});

function unmountNow(): void {
  if (component) void unmount(component);
  component = null;
  target.replaceChildren();
}
