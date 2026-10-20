// @vitest-environment jsdom
import "../testing/media";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

// jsdom has no layout: the frame's size watcher sees nothing (the real web views have one)
globalThis.ResizeObserver ??= class {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
};

const { default: FrameHost } = await import("../testing/FrameHost.svelte");
const { default: FocusExit } = await import("./FocusExit.svelte");
const { ui, FOCUS_MOVE_MS } = await import("./ui.svelte");
const { ContributionRegistry } = await import("../plugins/registry.svelte");
const { registerCoreCommands } = await import("../commands/core");
const { runnable } = await import("../commands/commands");
const { EditingSession } = await import("../editing/session.svelte");
const { documents } = await import("../doc/document.svelte");

/** A source file of the app (the tests run from its folder). */
const source = (path: string): string => readFileSync(resolve(process.cwd(), "src", path), "utf8");

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
  ui.resetFocus();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("the focus state of a window", () => {
  it("starts off, toggles, and is not stored", () => {
    expect(ui.focus).toBe("off");
    ui.toggleFocus();
    expect(ui.focus).toBe("on");
    ui.toggleFocus();
    expect(ui.focus).toBe("off");
  });

  it("moves for a moment after a toggle, then rests", () => {
    vi.useFakeTimers();
    ui.setFocus("on");
    expect(ui.focusMoving).toBe(true);
    vi.advanceTimersByTime(FOCUS_MOVE_MS - 1);
    expect(ui.focusMoving).toBe(true);
    vi.advanceTimersByTime(2);
    expect(ui.focusMoving).toBe(false);
    // the same state again moves nothing
    ui.setFocus("on");
    expect(ui.focusMoving).toBe(false);
  });

  it("a storm of toggles ends in the last state with no timer left", () => {
    vi.useFakeTimers();
    for (let n = 0; n < 101; n++) ui.toggleFocus();
    expect(ui.focus).toBe("on");
    expect(ui.focusMoving).toBe(true);
    vi.advanceTimersByTime(FOCUS_MOVE_MS + 1);
    expect(ui.focusMoving).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("reset ends it at once, with no motion and no timer left", () => {
    vi.useFakeTimers();
    ui.setFocus("on");
    ui.resetFocus();
    expect(ui.focus).toBe("off");
    expect(ui.focusMoving).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("the command", () => {
  const context = (workspace: "welcome" | "project" | "singleFile" | "notes") =>
    ({
      workspace,
      git: { status: "noRepo" },
      permark: "notInitialized",
      agent: { status: "off" },
      liveUpdates: { status: "available" },
      distribution: "direct",
      platform: "windows",
      plugins: [],
    }) as never;

  it("is Mod+Alt+F, in the palette, and needs a project, a file or the Notes", () => {
    const registry = new ContributionRegistry();
    const off = registerCoreCommands(registry);
    const command = registry.of("command").find((c) => c.id === "doc.focus");
    const key = registry.of("shortcut").find((s) => s.command === "doc.focus");
    expect(command?.label).toBe("cmd.focus");
    expect(key?.keys).toBe("Mod+Alt+f");
    expect(command?.inPalette).not.toBe(false);
    vi.spyOn(documents, "path", "get").mockReturnValue("a.md");
    expect(command && runnable(command, context("welcome"))).toBe(false);
    off();
  });

  it("runs with a document shown, and always while it is on (the way out stays)", () => {
    const registry = new ContributionRegistry();
    const off = registerCoreCommands(registry);
    const command = registry.of("command").find((x) => x.id === "doc.focus");
    if (!command) throw new Error("no command");
    for (const kind of ["project", "singleFile", "notes"] as const) {
      vi.spyOn(documents, "path", "get").mockReturnValue(null);
      expect(runnable(command, context(kind)), `${kind} without a document`).toBe(false);
      vi.spyOn(documents, "path", "get").mockReturnValue("a.md");
      expect(runnable(command, context(kind)), `${kind} with a document`).toBe(true);
    }
    vi.spyOn(documents, "path", "get").mockReturnValue(null);
    ui.setFocus("on");
    expect(runnable(command, context("project"))).toBe(true);
    void command.run();
    expect(ui.focus).toBe("off");
    off();
  });
});

describe("the frame", () => {
  const region = (selector: string): HTMLElement | null => target.querySelector(selector);

  it("shows every region while focus mode is off", () => {
    component = mount(FrameHost, { target, props: {} });
    flushSync();
    expect(region(".frame")?.classList.contains("focus")).toBe(false);
    for (const selector of [".top", ".sidebar", ".panel", ".status"]) {
      expect(region(selector)?.inert, selector).toBe(false);
    }
    expect(target.querySelectorAll("[role=separator]")).toHaveLength(2);
  });

  it("in focus mode hides every region but the page: inert, still mounted, no splitters", () => {
    component = mount(FrameHost, { target, props: { focus: true } });
    flushSync();
    expect(region(".frame")?.classList.contains("focus")).toBe(true);
    for (const selector of [".top", ".sidebar", ".panel", ".status"]) {
      expect(region(selector)?.inert, selector).toBe(true);
    }
    // the regions keep their content (state survives); the page is not inert
    expect(region("#side-button")).not.toBeNull();
    expect(region("#right-text")?.textContent).toBe("right");
    expect(Boolean(region(".main")?.inert)).toBe(false);
    expect(region("#page-text")?.textContent).toBe("the page");
    expect(target.querySelectorAll("[role=separator]")).toHaveLength(0);
  });

  it("the tracks transition only while moving", () => {
    component = mount(FrameHost, { target, props: { focus: true, moving: true } });
    flushSync();
    expect(region(".frame")?.classList.contains("moving")).toBe(true);
  });

  it("works with one side or none", () => {
    component = mount(FrameHost, { target, props: { focus: true, sidebar: false, panel: false } });
    flushSync();
    expect(region(".sidebar")).toBeNull();
    expect(region(".panel")).toBeNull();
    expect(region(".top")?.inert).toBe(true);
  });

  it("respects the wish for less motion: every transition of the regions is off then", () => {
    const css = source("core/shell/Frame.svelte");
    const reduced = css.slice(css.indexOf("@media (prefers-reduced-motion: reduce)"));
    for (const selector of [".frame.focus .top", ".frame.focus .sidebar", ".frame.moving .body"]) {
      expect(reduced, selector).toContain(selector);
    }
    expect(reduced).toContain("transition: none");
    const area = source("core/doc/DocumentArea.svelte");
    expect(area.slice(area.indexOf("prefers-reduced-motion"))).toContain("transition: none");
  });
});

describe("the way out", () => {
  const button = (): HTMLButtonElement => target.querySelector(".leave") as HTMLButtonElement;
  const shown = (): boolean => target.querySelector(".exit")?.classList.contains("shown") ?? false;
  const pointer = (x: number, y: number): void => {
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: x, clientY: y }));
    flushSync();
  };

  beforeEach(() => {
    Object.defineProperty(window, "innerWidth", { value: 1000, configurable: true });
    ui.setFocus("on");
  });

  it("shows for the first moments, then only when the pointer comes near the corner", async () => {
    vi.useFakeTimers();
    component = mount(FocusExit, { target, props: {} });
    flushSync();
    expect(shown()).toBe(true);
    vi.advanceTimersByTime(2400);
    flushSync();
    expect(shown()).toBe(true);
    vi.advanceTimersByTime(200);
    flushSync();
    expect(shown()).toBe(false);
    pointer(500, 10);
    expect(shown()).toBe(false);
    pointer(900, 200);
    expect(shown()).toBe(false);
    pointer(900, 10);
    expect(shown()).toBe(true);
    pointer(839, 10);
    expect(shown()).toBe(false);
    pointer(1000, 64);
    expect(shown()).toBe(true);
    pointer(1000, 65);
    expect(shown()).toBe(false);
    pointer(900, 10);
    document.documentElement.dispatchEvent(new Event("pointerleave"));
    flushSync();
    expect(shown()).toBe(false);
    await tick();
  });

  it("shows while the button has the keyboard focus and leaves focus mode on click", () => {
    vi.useFakeTimers();
    component = mount(FocusExit, { target, props: {} });
    flushSync();
    vi.advanceTimersByTime(3000);
    flushSync();
    expect(shown()).toBe(false);
    button().focus();
    flushSync();
    expect(shown()).toBe(true);
    button().blur();
    flushSync();
    expect(shown()).toBe(false);
    button().click();
    expect(ui.focus).toBe("off");
  });

  it("is named for the screen reader and says the key", () => {
    component = mount(FocusExit, { target, props: {} });
    flushSync();
    expect(button().getAttribute("aria-label")).toContain("Leave focus mode");
    expect(button().getAttribute("title")).toBe(button().getAttribute("aria-label"));
  });

  it("takes no clicks while it is not shown (the page under the corner stays clickable)", () => {
    const css = source("core/shell/FocusExit.svelte");
    expect(css).toMatch(/\.exit \{[^}]*pointer-events: none/);
    expect(css).toMatch(/\.shown \.leave \{[^}]*pointer-events: auto/);
  });

  it("carries a small dot while the editor holds unsaved changes", async () => {
    const spy = vi.spyOn(EditingSession.prototype, "unsaved", "get");
    spy.mockReturnValue(false);
    component = mount(FocusExit, { target, props: {} });
    flushSync();
    expect(target.querySelector(".dot")).toBeNull();
    await unmount(component);
    spy.mockReturnValue(true);
    component = mount(FocusExit, { target, props: {} });
    flushSync();
    expect(target.querySelector(".dot")?.getAttribute("aria-label")).toBe("Unsaved changes");
  });

  it("removes its listeners and timer when it goes away", async () => {
    vi.useFakeTimers();
    const remove = vi.spyOn(window, "removeEventListener");
    component = mount(FocusExit, { target, props: {} });
    flushSync();
    await unmount(component);
    component = null;
    expect(remove).toHaveBeenCalledWith("pointermove", expect.any(Function));
    expect(vi.getTimerCount()).toBe(0);
  });
});
