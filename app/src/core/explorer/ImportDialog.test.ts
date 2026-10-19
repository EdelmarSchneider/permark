// @vitest-environment jsdom
import "../testing/media";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import type { ConflictChoice, ImportMode, ImportPlan, ImportProgress } from "../../api";
import ImportDialog from "./ImportDialog.svelte";
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
  setMode: Mock<(mode: ImportMode) => void>;
  setConflicts: Mock<(conflicts: ConflictChoice) => void>;
  confirm: Mock<() => void>;
  cancel: Mock<() => void>;
};

function open(
  p: ImportPlan,
  mode: ImportMode = "copy",
  conflicts: ConflictChoice = "keepBoth",
  where = "docs",
): Spies {
  const spies: Spies = {
    setMode: vi.fn<(mode: ImportMode) => void>(),
    setConflicts: vi.fn<(conflicts: ConflictChoice) => void>(),
    confirm: vi.fn<() => void>(),
    cancel: vi.fn<() => void>(),
  };
  component = mount(ImportDialog, {
    target,
    props: { plan: p, where, mode, conflicts, ...spies },
  });
  flushSync();
  return spies;
}

const text = (): string => target.textContent ?? "";
const buttons = (): HTMLButtonElement[] => [...target.querySelectorAll("button")];
const named = (name: string): HTMLButtonElement | undefined =>
  buttons().find((b) => b.textContent?.trim() === name);

describe("the question", () => {
  it("names how many and where, with Copy | Move and the originals' fate", () => {
    open(plan({ items: [item("a.md"), item("b.md"), item("c")] }));
    expect(target.querySelector("h2")?.textContent).toBe("Add 3 items to “docs”");
    expect(target.querySelector("[role=dialog]")?.getAttribute("aria-modal")).toBe("true");
    expect(named("Copy")).toBeDefined();
    expect(named("Move")).toBeDefined();
    expect(text()).toContain("The originals stay where they are.");
    expect(target.querySelector(".taken")).toBeNull();
  });

  it("says one item in the singular", () => {
    open(plan());
    expect(target.querySelector("h2")?.textContent).toBe("Add 1 item to “docs”");
  });

  it("lists the taken names first, with what to do about them", () => {
    const spies = open(
      plan({
        items: [
          item("a.md", { conflict: true }),
          item("pics", { conflict: true, kind: "folder" }),
          item("b.md"),
        ],
      }),
    );
    const section = target.querySelector(".taken");
    expect(section?.textContent).toContain("2 names already exist here");
    expect(section?.textContent).toContain("a.md · pics/");
    expect(section?.textContent).not.toContain("b.md");
    // the taken names come before the choice of Copy | Move
    const html = target.innerHTML;
    expect(html.indexOf("already exist")).toBeLessThan(html.indexOf("How to add them"));
    expect(text()).toContain("The new one gets a number, like name (2).");
    named("Skip those")?.click();
    expect(spies.setConflicts).toHaveBeenCalledWith("skip");
  });

  it("cuts a long list of taken names after six", () => {
    const items = Array.from({ length: 9 }, (_, n) => item(`f${n}.md`, { conflict: true }));
    open(plan({ items }));
    const section = target.querySelector(".taken")?.textContent ?? "";
    expect(section).toContain("9 names already exist here");
    expect(section).toContain("f5.md");
    expect(section).not.toContain("f6.md");
    expect(section).toContain("and 3 more");
  });

  it("explains Skip in its words", () => {
    open(plan({ items: [item("a.md", { conflict: true })] }), "copy", "skip");
    expect(text()).toContain("Those items are left out. Nothing is replaced.");
  });

  it("offers Move only when nothing lies in the project already, and says why", () => {
    open(plan({ items: [item("a.md"), item("b.md", { inProject: true })] }));
    expect(named("Move")).toBeUndefined();
    expect(text()).toContain("Move them in the Explorer");
  });

  it("the choice of mode reaches the owner and names the primary button", () => {
    const spies = open(plan(), "move");
    expect(buttons().filter((b) => b.textContent?.trim() === "Move")).toHaveLength(2);
    named("Copy")?.click();
    expect(spies.setMode).toHaveBeenCalledWith("copy");
    const primary = target.querySelector<HTMLButtonElement>(".btn--primary");
    expect(primary?.textContent?.trim()).toBe("Move");
    primary?.click();
    expect(spies.confirm).toHaveBeenCalledTimes(1);
  });

  it("lists what cannot come in, quietly, four at most", () => {
    const refused = ["a", "b", "c", "d", "e"].map((name) => ({
      source: `C:/out/${name}`,
      name,
      reason: "link" as const,
    }));
    open(plan({ refused }));
    const section = target.querySelector(".refused")?.textContent ?? "";
    expect(section).toContain("5 items can't be added");
    expect(section).toContain("is a link, and links aren't followed");
    expect(section).toContain("d is a link");
    expect(section).not.toContain("e is a link");
    expect(section).toContain("and 1 more");
  });

  it("starts with the focus on the answer that takes nothing away", () => {
    open(plan());
    expect(document.activeElement?.textContent?.trim()).toBe("Copy");
    expect(document.activeElement?.classList.contains("btn--primary")).toBe(true);
  });

  it("Escape, Cancel and the backdrop cancel; a click inside does not", () => {
    const spies = open(plan());
    const dialog = target.querySelector<HTMLElement>("[role=dialog]");
    dialog?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(spies.cancel).toHaveBeenCalledTimes(1);
    named("Cancel")?.click();
    expect(spies.cancel).toHaveBeenCalledTimes(2);
    target.querySelector<HTMLElement>(".backdrop")?.click();
    expect(spies.cancel).toHaveBeenCalledTimes(3);
    dialog?.click();
    expect(spies.cancel).toHaveBeenCalledTimes(3);
    expect(spies.confirm).not.toHaveBeenCalled();
  });

  it("keeps Tab inside the dialog", () => {
    open(plan({ items: [item("a.md", { conflict: true })] }));
    const dialog = target.querySelector<HTMLElement>("[role=dialog]");
    const focusable = [
      ...(dialog?.querySelectorAll<HTMLElement>("button:not(:disabled):not([tabindex='-1'])") ??
        []),
    ];
    const last = focusable.at(-1);
    last?.focus();
    const tab = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
    dialog?.dispatchEvent(tab);
    expect(tab.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(focusable[0]);
  });

  it("names and CJK or spaced folders come out as they are", () => {
    open(
      plan({ items: [item("文档 ü.md", { conflict: true })] }),
      "copy",
      "keepBoth",
      "测试 文件夹",
    );
    expect(target.querySelector("h2")?.textContent).toBe("Add 1 item to “测试 文件夹”");
    expect(text()).toContain("文档 ü.md");
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
