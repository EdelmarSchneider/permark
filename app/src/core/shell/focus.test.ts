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

// jsdom has no pointer capture: a handle that is dragged asks for it
Element.prototype.setPointerCapture ??= () => {};
Element.prototype.releasePointerCapture ??= () => {};

const { default: FrameHost } = await import("../testing/FrameHost.svelte");
const { default: FocusExit } = await import("./FocusExit.svelte");
const { BOTTOM, PAGE_MIN_H } = await import("./layout");
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
    expect(ui.frameMoving).toBe(true);
    vi.advanceTimersByTime(FOCUS_MOVE_MS - 1);
    expect(ui.frameMoving).toBe(true);
    vi.advanceTimersByTime(2);
    expect(ui.frameMoving).toBe(false);
    // the same state again moves nothing
    ui.setFocus("on");
    expect(ui.frameMoving).toBe(false);
  });

  it("a storm of toggles ends in the last state with no timer left", () => {
    vi.useFakeTimers();
    for (let n = 0; n < 101; n++) ui.toggleFocus();
    expect(ui.focus).toBe("on");
    expect(ui.frameMoving).toBe(true);
    vi.advanceTimersByTime(FOCUS_MOVE_MS + 1);
    expect(ui.frameMoving).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("reset ends it at once, with no motion and no timer left", () => {
    vi.useFakeTimers();
    ui.setFocus("on");
    ui.resetFocus();
    expect(ui.focus).toBe("off");
    expect(ui.frameMoving).toBe(false);
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
      agentFiles: [],
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

  it("names its side regions for assistive tech: the sidebar, the right panel", () => {
    component = mount(FrameHost, { target, props: {} });
    flushSync();
    expect(region(".sidebar")?.tagName).toBe("ASIDE");
    expect(region(".sidebar")?.getAttribute("aria-label")).toBe("Sidebar");
    expect(region(".panel")?.tagName).toBe("ASIDE");
    expect(region(".panel")?.getAttribute("aria-label")).toBe("Right panel");
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

  it("a side panel the person hid: inert, still mounted, no splitter, no room; the other one stays", () => {
    component = mount(FrameHost, { target, props: { sidebarHidden: true } });
    flushSync();
    const body = region(".body")!;
    expect(region(".sidebar")?.inert).toBe(true);
    expect(region(".sidebar")?.classList.contains("gone")).toBe(true);
    expect(region("#side-button")).not.toBeNull();
    expect(body.style.getPropertyValue("--w-sidebar")).toBe("0px");
    expect(region(".panel")?.inert).toBe(false);
    expect(body.style.getPropertyValue("--w-right")).not.toBe("0px");
    expect(target.querySelectorAll("[role=separator]")).toHaveLength(1);
    expect(region(".top")?.inert).toBe(false);
  });

  it("both hidden: only the page between the bars; focus mode on top keeps them hidden", async () => {
    component = mount(FrameHost, { target, props: { sidebarHidden: true, rightHidden: true } });
    flushSync();
    expect(region(".sidebar")?.inert).toBe(true);
    expect(region(".panel")?.inert).toBe(true);
    expect(region(".body")!.style.getPropertyValue("--w-right")).toBe("0px");
    expect(target.querySelectorAll("[role=separator]")).toHaveLength(0);
    expect(region(".status")?.inert).toBe(false);
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
    for (const selector of [
      ".frame.focus .top",
      ".sidebar.gone",
      ".panel.gone",
      ".bottom.gone",
      ".frame.moving .body",
      ".frame.moving .center",
    ]) {
      expect(reduced, selector).toContain(selector);
    }
    expect(reduced).toContain("transition: none");
    const area = source("core/doc/DocumentArea.svelte");
    expect(area.slice(area.indexOf("prefers-reduced-motion"))).toContain("transition: none");
  });
});

describe("the bottom region of the frame", () => {
  const region = (selector: string): HTMLElement | null => target.querySelector(selector);
  const bottom = (): HTMLElement | null => region('[data-region="bottom"]');
  /** The room the center column gives the bottom region. */
  const track = (): string => region(".center")?.style.getPropertyValue("--h-bottom") ?? "";
  const bar = (): HTMLElement | null => region('[role=separator][data-side="bottom"]');
  /** The center column is this high (jsdom has no layout). */
  const high = (px: number): void => {
    vi.spyOn(Element.prototype, "clientHeight", "get").mockReturnValue(px);
  };

  it("is absent while nothing is drawn there: no band, no splitter, the page alone in the center column", () => {
    component = mount(FrameHost, { target, props: {} });
    flushSync();
    expect(bottom()).toBeNull();
    expect(bar()).toBeNull();
    expect(track()).toBe("");
    expect(region(".main")?.parentElement).toBe(region(".center"));
    expect(target.querySelectorAll("[role=separator]")).toHaveLength(2);
  });

  it("sits under the page, between the side columns, at the height asked for; its splitter lies on its top edge", () => {
    component = mount(FrameHost, { target, props: { bottom: true, bottomHeight: 300 } });
    flushSync();
    expect(bottom()?.tagName).toBe("ASIDE");
    expect(bottom()?.getAttribute("aria-label")).toBe("Bottom panel");
    expect(bottom()?.inert).toBe(false);
    expect(track()).toBe("300px");
    // the Tab order: sidebar, page, bottom, right
    expect(
      [...target.querySelectorAll("[data-region]")].map((el) => el.getAttribute("data-region")),
    ).toEqual(["sidebar", "page", "bottom", "right"]);
    // in the center column: the side columns keep the whole height
    expect(bottom()?.parentElement).toBe(region(".center"));
    expect(region(".sidebar")?.parentElement).toBe(region(".body"));
    expect(region(".panel")?.parentElement).toBe(region(".body"));
    expect(bar()?.parentElement).toBe(region(".center"));
    expect(bar()?.getAttribute("aria-orientation")).toBe("horizontal");
    expect(bar()?.getAttribute("aria-label")).toBe("Resize the bottom panel");
    expect(bar()?.getAttribute("aria-valuenow")).toBe("300");
    expect(bar()?.getAttribute("aria-valuemin")).toBe(String(BOTTOM.min));
    // the side panels' handles stay upright
    expect(
      [...target.querySelectorAll('[role=separator]:not([data-side="bottom"])')].map((el) =>
        el.getAttribute("aria-orientation"),
      ),
    ).toEqual(["vertical", "vertical"]);
  });

  it("starts at its default height", () => {
    component = mount(FrameHost, { target, props: { bottom: true } });
    flushSync();
    expect(track()).toBe(`${BOTTOM.def}px`);
  });

  it("gives way to the page in a short window: the page keeps its minimum height", () => {
    high(PAGE_MIN_H + 180);
    component = mount(FrameHost, { target, props: { bottom: true, bottomHeight: 240 } });
    flushSync();
    expect(track()).toBe("180px");
    expect(bottom()?.inert).toBe(false);
    expect(bar()?.getAttribute("aria-valuenow")).toBe("180");
    // no taller than the window lets it be
    expect(bar()?.getAttribute("aria-valuemax")).toBe("180");
  });

  it("a window too short for even its minimum: not drawn (no room, inert, no splitter), still mounted", () => {
    high(PAGE_MIN_H + BOTTOM.min - 1);
    component = mount(FrameHost, { target, props: { bottom: true, bottomHeight: 240 } });
    flushSync();
    expect(track()).toBe("0px");
    expect(bottom()?.inert).toBe(true);
    expect(bottom()?.classList.contains("gone")).toBe(true);
    expect(region("#bottom-button")).not.toBeNull();
    expect(bar()).toBeNull();
    // the side panels are as they were
    expect(region(".sidebar")?.inert).toBe(false);
    expect(target.querySelectorAll("[role=separator]")).toHaveLength(2);
  });

  it("says how high its center column is, with or without a bottom region: the window decides what fits", () => {
    high(640);
    const heights: number[] = [];
    component = mount(FrameHost, {
      target,
      props: { onCenterHeight: (px: number) => heights.push(px) },
    });
    flushSync();
    expect(heights.at(-1)).toBe(640);
  });

  it("draws the height it is given, never one of its own: the fit is its owner's", () => {
    const frame = source("core/shell/Frame.svelte");
    expect(frame).not.toContain("fitBottom");
    expect(frame).toContain("onCenterHeight");
  });

  it("hidden by the person: no room, inert, still mounted, no splitter; the side panels stay", () => {
    component = mount(FrameHost, { target, props: { bottom: true, bottomHidden: true } });
    flushSync();
    expect(track()).toBe("0px");
    expect(bottom()?.inert).toBe(true);
    expect(bottom()?.classList.contains("gone")).toBe(true);
    expect(region("#bottom-button")).not.toBeNull();
    expect(bar()).toBeNull();
    expect(region(".sidebar")?.inert).toBe(false);
    expect(region(".panel")?.inert).toBe(false);
    expect(target.querySelectorAll("[role=separator]")).toHaveLength(2);
  });

  it("focus mode folds it with the side panels: only the page", () => {
    component = mount(FrameHost, { target, props: { bottom: true, focus: true } });
    flushSync();
    expect(track()).toBe("0px");
    expect(bottom()?.inert).toBe(true);
    expect(region("#bottom-button")).not.toBeNull();
    expect(Boolean(region(".main")?.inert)).toBe(false);
    expect(target.querySelectorAll("[role=separator]")).toHaveLength(0);
  });

  it("its splitter: Up | Down by 16 | 64 with Shift, Home | End = the limits, Enter | double click = the default", () => {
    high(700);
    const asked: number[] = [];
    component = mount(FrameHost, {
      target,
      props: { bottom: true, bottomHeight: 300, onBottomHeight: (px: number) => asked.push(px) },
    });
    flushSync();
    const key = (name: string, shiftKey = false): number | undefined => {
      const event = new KeyboardEvent("keydown", {
        key: name,
        shiftKey,
        bubbles: true,
        cancelable: true,
      });
      bar()?.dispatchEvent(event);
      return event.defaultPrevented ? asked.at(-1) : undefined;
    };
    expect(key("ArrowUp")).toBe(316);
    expect(key("ArrowDown")).toBe(284);
    expect(key("ArrowUp", true)).toBe(364);
    expect(key("ArrowDown", true)).toBe(236);
    expect(key("Home")).toBe(BOTTOM.min);
    // the tallest the window gives: the page keeps its minimum
    expect(key("End")).toBe(700 - PAGE_MIN_H);
    expect(key("Enter")).toBe(BOTTOM.def);
    // across the bar: not its keys
    expect(key("ArrowLeft")).toBeUndefined();
    expect(key("ArrowRight")).toBeUndefined();
    bar()?.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    expect(asked.at(-1)).toBe(BOTTOM.def);
    expect(asked).toHaveLength(8);
  });

  it("its splitter dragged: up makes it taller, down shorter, never taller than the window gives", () => {
    high(700);
    const asked: number[] = [];
    component = mount(FrameHost, {
      target,
      props: { bottom: true, bottomHeight: 300, onBottomHeight: (px: number) => asked.push(px) },
    });
    flushSync();
    const pointer = (type: string, clientX: number, clientY: number): void => {
      bar()?.dispatchEvent(
        new PointerEvent(type, { button: 0, pointerId: 1, clientX, clientY, bubbles: true }),
      );
    };
    // not held: a pointer passing over it changes nothing
    pointer("pointermove", 400, 380);
    expect(asked).toEqual([]);
    pointer("pointerdown", 400, 400);
    pointer("pointermove", 400, 360);
    expect(asked.at(-1)).toBe(340);
    pointer("pointermove", 400, 450);
    expect(asked.at(-1)).toBe(250);
    // sideways: nothing of its own
    pointer("pointermove", 900, 450);
    expect(asked.at(-1)).toBe(250);
    pointer("pointermove", 400, -500);
    expect(asked.at(-1)).toBe(700 - PAGE_MIN_H);
    pointer("pointerup", 400, -500);
    const count = asked.length;
    pointer("pointermove", 400, 300);
    expect(asked).toHaveLength(count);
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
