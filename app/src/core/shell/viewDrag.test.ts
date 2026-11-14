// @vitest-environment jsdom
// A view dragged by its tab (or its header while alone in its group) with the mouse | a pen: a press becomes a drag
// after 6 px, a chip follows the pointer and ONE place to drop shows; a drop makes the move the views' menu makes
// (the same view, the same words, the keyboard on its options); Escape, a drop elsewhere, a lost pointer or a window
// that loses the focus change nothing. Nothing of a drag is in the page while none runs.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Api } from "../../api";
import type { PluginId } from "../../api/generated/PluginId";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});
// jsdom has no layout: a region's size watcher sees nothing
globalThis.ResizeObserver ??= class {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
};
// jsdom has no pointer capture: a drag asks for it
Element.prototype.setPointerCapture ??= () => {};
Element.prototype.releasePointerCapture ??= () => {};
Element.prototype.hasPointerCapture ??= () => false;

const { createMockApi } = await import("../../api/mock");
const { NO_REQUIREMENT } = await import("../conditions/available");
const { conditions, neutralContext } = await import("../conditions/context.svelte");
const { contributions } = await import("../plugins/instance.svelte");
const { default: RegionsHost } = await import("../testing/RegionsHost.svelte");
const { probe } = await import("../testing/ViewProbe.svelte");
const { BOTTOM, PAGE_MIN_H } = await import("./layout");
const { layout } = await import("./layout.svelte");
const { arrangement } = await import("./arrangement.svelte");
const { FILES, OUTLINE, REGIONS, SEARCH } = await import("./arrangement");
const { viewDrag } = await import("./viewDrag.svelte");
const { viewMenu } = await import("./viewMenu.svelte");

/** A right panel with a header of its own (the probe), a second right panel. */
const PROBE = "probe.panel";
const OTHER = "other.panel";

let api: Api;
let component: Record<string, unknown> | null = null;
let stop: () => void = () => {};
const removals: (() => void)[] = [];

function panel(id: string, view: () => Promise<unknown>): void {
  removals.push(
    contributions.register({
      kind: "panel",
      id,
      owner: "test" as PluginId,
      side: "right",
      label: id === PROBE ? "outline.tab" : "live.label",
      requires: NO_REQUIREMENT,
      view: view as never,
    }),
  );
}

type Groups = readonly (readonly string[])[];
/** The arrangement with these groups (equal shares) left, right and at the bottom. */
function arrange(left: Groups, right: Groups, bottom: Groups = []): void {
  const groups = (list: Groups) => list.map((views) => ({ views, share: 1 / list.length }));
  arrangement.arrange({ v: 2, left: groups(left), right: groups(right), bottom: groups(bottom) });
  flushSync();
}

/** Where the views of these tests sit now, region by region (the outline, not present here, is left out). */
const placed = (): Record<string, string[][]> =>
  Object.fromEntries(
    REGIONS.map((region) => [
      region,
      arrangement.saved[region]
        .map((g) => g.views.filter((id) => id !== OUTLINE))
        .filter((views) => views.length > 0),
    ]),
  );

const hostOf = (id: string): HTMLElement | null =>
  document.querySelector<HTMLElement>(`[data-view-host="${id}"]`);
const tabOf = (id: string): HTMLElement => {
  const tab = document.querySelector<HTMLElement>(`[role="tab"][data-view="${id}"]`);
  if (tab === null) throw new Error(`no tab ${id}`);
  return tab;
};
/** The title in the header a view draws for itself. */
const titleOf = (id: string): HTMLElement => {
  const title = hostOf(id)?.querySelector<HTMLElement>("[data-view-header] h2");
  if (!title) throw new Error(`no header ${id}`);
  return title;
};
const headerButton = (id: string): HTMLElement | null =>
  hostOf(id)?.querySelector<HTMLElement>(`[data-view-menu="${id}"]`) ?? null;
/** The options buttons at the end of the tab strips of a region. */
const stripButtons = (region: string): HTMLElement[] =>
  [...document.querySelectorAll<HTMLElement>(`[data-region="${region}"] [data-view-menu]`)].filter(
    (button) => button.closest("[data-view-host]") === null,
  );
const said = (): string => document.querySelector("[data-view-status]")?.textContent ?? "";

const layer = (): HTMLElement | null =>
  document.querySelector<HTMLElement>("[data-view-drag-layer]");
const ghost = (): HTMLElement | null => document.querySelector<HTMLElement>("[data-drag-ghost]");
const targets = (): HTMLElement[] => [
  ...document.querySelectorAll<HTMLElement>("[data-drag-target]"),
];
const root = document.documentElement;

type Sides = readonly [left: number, top: number, right: number, bottom: number];
/**
 * jsdom has no layout: the boxes of what the window draws, by what an element is (the first rule that fits; an
 * element no rule names has an empty box).
 */
function lay(rules: readonly (readonly [string | ((el: Element) => boolean), Sides])[]): void {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
    const rule = rules.find(([is]) => (typeof is === "string" ? this.matches(is) : is(this)));
    const [left, top, right, bottom] = rule?.[1] ?? [0, 0, 0, 0];
    return {
      left,
      top,
      right,
      bottom,
      x: left,
      y: top,
      width: right - left,
      height: bottom - top,
      toJSON: () => ({}),
    };
  });
}

/** The strip that holds the tab of `id`. */
const stripWith =
  (id: string) =>
  (el: Element): boolean =>
    el.matches("[data-view-strip]") && el.querySelector(`[data-view="${id}"]`) !== null;

// a window of 1000 x 700: Files | Search as tabs at the left, the probe over the other panel at the right
const FRAME = [
  ['[data-frame="body"]', [0, 42, 1000, 676]],
  ['[data-frame="center"]', [260, 42, 720, 676]],
] as const;
const START = [
  ...FRAME,
  [stripWith(FILES), [0, 42, 260, 80]],
  [`[role="tab"][data-view="${FILES}"]`, [8, 50, 68, 80]],
  [`[role="tab"][data-view="${SEARCH}"]`, [70, 50, 140, 80]],
  [`[data-view-slot="${FILES}"]`, [0, 80, 260, 676]],
  [`[data-view-slot="${SEARCH}"]`, [0, 80, 260, 676]],
  [`[data-view-slot="${PROBE}"]`, [720, 42, 1000, 356]],
  [`[data-view-slot="${OTHER}"]`, [720, 363, 1000, 676]],
] as const;

function pointer(
  type: string,
  on: Element,
  x: number,
  y: number,
  more: PointerEventInit = {},
): PointerEvent {
  const event = new PointerEvent(type, {
    bubbles: true,
    cancelable: true,
    button: 0,
    // the main button is held from the press until the release
    buttons: type === "pointerdown" || type === "pointermove" ? 1 : 0,
    pointerId: 1,
    pointerType: "mouse",
    isPrimary: true,
    clientX: x,
    clientY: y,
    ...more,
  });
  on.dispatchEvent(event);
  flushSync();
  return event;
}

/** The click a webview makes of a press and its release with the pointer (one a key makes has `detail` 0: what
 *  `el.click()` gives). */
function mouseClick(on: Element): void {
  on.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, detail: 1 }));
  flushSync();
}

type At = readonly [x: number, y: number];
/** Presses `handle` and moves the pointer to `to`. */
function drag(handle: Element, from: At, to: At, more: PointerEventInit = {}): void {
  pointer("pointerdown", handle, from[0], from[1], more);
  pointer("pointermove", handle, to[0], to[1], more);
}

/** The pointer let go at `at`; what the drop changed is drawn and the keyboard has settled. */
async function release(handle: Element, at: At): Promise<void> {
  pointer("pointerup", handle, at[0], at[1]);
  await tick();
  await tick();
}

/** The turn after a release: the click it made has come and gone. */
const later = (): Promise<void> => new Promise((done) => setTimeout(done, 0));

/** The window's regions on screen, the probe's view loaded. */
async function show(): Promise<HTMLElement> {
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(RegionsHost, { target });
  flushSync();
  await vi.waitFor(() => expect(hostOf(PROBE)?.querySelector("[data-probe]")).toBeTruthy());
  const host = hostOf(PROBE);
  if (host === null) throw new Error("no probe view");
  return host;
}

beforeEach(async () => {
  document.body.replaceChildren();
  probe.mounts = 0;
  api = createMockApi();
  panel(PROBE, () => import("../testing/ViewProbe.svelte"));
  panel(OTHER, () => import("../testing/StripFirst.svelte"));
  stop = arrangement.start(api);
  conditions.set({ ...neutralContext(), workspace: "project" });
  await api.projectSessionSet("layout.views", null);
  await layout.load(api, true);
  layout.setOpen(PROBE, true);
  layout.setOpen(OTHER, true);
  arrange([[FILES, SEARCH]], [[PROBE], [OTHER]]);
  lay(START);
});

afterEach(async () => {
  viewDrag.cancel();
  // a new press: what waited for the release of a drag that was cancelled is gone
  window.dispatchEvent(new PointerEvent("pointerdown"));
  await later();
  viewMenu.close();
  if (component) await unmount(component);
  component = null;
  stop();
  for (const remove of removals.splice(0)) remove();
  conditions.set(neutralContext());
  for (const region of REGIONS) arrangement.setSize(region, 0);
  layout.setCenterHeight(0);
  arrangement.say("");
  await layout.load(api, false);
  vi.restoreAllMocks();
});

describe("a press on a tab", () => {
  it("that moves less than 6 px stays a click: nothing is drawn, the tab is selected as ever", async () => {
    await show();
    const search = tabOf(SEARCH);
    pointer("pointerdown", search, 100, 60);
    pointer("pointermove", search, 104, 63);
    expect(viewDrag.state.status).toBe("pending");
    expect(layer()).toBeNull();
    expect(root.hasAttribute("data-view-drag")).toBe(false);
    pointer("pointerup", search, 104, 63);
    expect(viewDrag.state.status).toBe("idle");
    mouseClick(search);
    expect(arrangement.isSelected(SEARCH)).toBe(true);
  });

  it("becomes a drag at 6 px, with the mouse or a pen", async () => {
    await show();
    drag(tabOf(SEARCH), [100, 60], [106, 60]);
    expect(viewDrag.state.status).toBe("dragging");
    expect(layer()).not.toBeNull();
    viewDrag.cancel();
    flushSync();
    drag(tabOf(SEARCH), [100, 60], [100, 90], { pointerType: "pen" });
    expect(viewDrag.dragged).toBe(SEARCH);
  });

  it("never with a finger, another button, or a second pointer: the tab just works as a tab", async () => {
    await show();
    const search = tabOf(SEARCH);
    for (const more of [{ pointerType: "touch" }, { button: 2 }, { isPrimary: false }]) {
      drag(search, [100, 60], [500, 300], more);
      expect(viewDrag.state.status, JSON.stringify(more)).toBe("idle");
      expect(layer()).toBeNull();
      pointer("pointerup", search, 500, 300, more);
    }
    mouseClick(search);
    expect(arrangement.isSelected(SEARCH)).toBe(true);
  });
});

describe("while a view is dragged", () => {
  it("a chip with its name follows the pointer and ONE place to drop shows; the page knows a drag runs", async () => {
    await show();
    const search = tabOf(SEARCH);
    // the lower half of the probe's body
    drag(search, [100, 60], [800, 300]);
    expect(layer()?.getAttribute("aria-hidden")).toBe("true");
    expect(ghost()?.textContent?.trim()).toBe("Search");
    expect(ghost()?.querySelector("svg")).not.toBeNull();
    // the chip sits below-right of the pointer: what is under the pointer (a tab strip) stays readable
    expect(ghost()?.style.transform).toBe("translate(814px, 312px)");
    expect(targets().map((el) => el.getAttribute("data-drag-target"))).toEqual(["group"]);
    const [half] = targets();
    expect([half?.style.left, half?.style.top, half?.style.width, half?.style.height]).toEqual([
      "720px",
      "199px",
      "280px",
      "157px",
    ]);
    // the half says what a drop there does, in the menu's words
    expect(half?.textContent?.trim()).toBe("Show below");
    expect(root.getAttribute("data-view-drag")).toBe("target");
    expect(search.classList.contains("dragged")).toBe(true);
    expect(tabOf(FILES).classList.contains("dragged")).toBe(false);

    // over the page: no place to drop
    pointer("pointermove", search, 500, 300);
    expect(targets()).toEqual([]);
    expect(ghost()).not.toBeNull();
    expect(root.getAttribute("data-view-drag")).toBe("none");

    // the start of its own strip: a line before the first tab
    pointer("pointermove", search, 5, 60);
    expect(targets().map((el) => el.getAttribute("data-drag-target"))).toEqual(["tab"]);
    expect(targets()[0]?.style.width).toBe("2px");

    // the foot of the center column: the bottom region, named as the menu names the move
    pointer("pointermove", search, 500, 660);
    expect(targets().map((el) => el.getAttribute("data-drag-target"))).toEqual(["region"]);
    expect(targets()[0]?.textContent?.trim()).toBe("Move to bottom panel");
    // the words keep to the half of the edge the pointer (and the chip beside it) is not in
    const words = (): boolean | undefined =>
      targets()[0]?.querySelector(".words")?.classList.contains("first");
    expect(words()).toBe(true);
    pointer("pointermove", search, 300, 660);
    expect(words()).toBe(false);
  });

  it("the half of a view's body says the move in the menu's words, by the way its region stacks; a line between tabs says nothing", async () => {
    await show();
    const search = tabOf(SEARCH);
    const words = (): Element | null => targets()[0]?.querySelector(".words") ?? null;
    // the upper half of the probe's body, the pointer near its top: the words keep to its foot, clear of the chip
    drag(search, [100, 60], [800, 100]);
    expect(targets()[0]?.getAttribute("data-drag-target")).toBe("group");
    expect(words()?.textContent).toBe("Show above");
    expect(words()?.classList.contains("first")).toBe(false);
    // its lower half, the pointer low in it: the words go to its head
    pointer("pointermove", search, 800, 340);
    expect(words()?.textContent).toBe("Show below");
    expect(words()?.classList.contains("first")).toBe(true);
    // a place among tabs is a line: no room for words, none drawn
    pointer("pointermove", search, 5, 60);
    expect(targets()[0]?.getAttribute("data-drag-target")).toBe("tab");
    expect(words()).toBeNull();
    viewDrag.cancel();
    flushSync();

    // at the bottom the groups sit side by side: the halves are a left and a right one
    arrange([[FILES, SEARCH]], [[PROBE]], [[OTHER]]);
    lay([
      ...FRAME,
      [stripWith(FILES), [0, 42, 260, 80]],
      [`[role="tab"][data-view="${FILES}"]`, [8, 50, 68, 80]],
      [`[role="tab"][data-view="${SEARCH}"]`, [70, 50, 140, 80]],
      [`[data-view-slot="${FILES}"]`, [0, 80, 260, 676]],
      [`[data-view-slot="${SEARCH}"]`, [0, 80, 260, 676]],
      [`[data-view-slot="${PROBE}"]`, [720, 42, 1000, 676]],
      [`[data-view-slot="${OTHER}"]`, [260, 436, 720, 676]],
    ]);
    drag(search, [100, 60], [300, 500]);
    expect(words()?.textContent).toBe("Show on the left");
    pointer("pointermove", search, 700, 500);
    expect(words()?.textContent).toBe("Show on the right");
  });

  it("the chip keeps inside the window: at its right | lower edge it goes to the other side of the pointer", async () => {
    await show();
    const search = tabOf(SEARCH);
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(120);
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(26);
    drag(search, [100, 60], [500, 300]);
    expect(ghost()?.style.transform).toBe("translate(514px, 312px)");
    // jsdom's window: 1024 x 768
    pointer("pointermove", search, 1000, 300);
    expect(ghost()?.style.transform).toBe("translate(866px, 312px)");
    pointer("pointermove", search, 500, 760);
    expect(ghost()?.style.transform).toBe("translate(514px, 722px)");
  });

  it("nothing of it stays in the page once it ends", async () => {
    await show();
    const search = tabOf(SEARCH);
    const before = arrangement.saved;
    drag(search, [100, 60], [800, 300]);
    await release(search, [500, 300]);
    expect(layer()).toBeNull();
    expect(ghost()).toBeNull();
    expect(targets()).toEqual([]);
    expect(root.hasAttribute("data-view-drag")).toBe(false);
    expect(search.classList.contains("dragged")).toBe(false);
    expect(viewDrag.state.status).toBe("idle");
    expect(arrangement.saved).toBe(before);
  });

  it("listens to the window only while it runs", async () => {
    await show();
    const added = vi.spyOn(window, "addEventListener");
    const removed = vi.spyOn(window, "removeEventListener");
    const search = tabOf(SEARCH);
    drag(search, [100, 60], [800, 300]);
    expect(added.mock.calls.length).toBeGreaterThan(0);
    await release(search, [500, 300]);
    await later();
    // the press, the drag, what it shows, the click it swallows: every listener added is taken off again
    const left = added.mock.calls.filter(
      ([type, listener, options]) =>
        !removed.mock.calls.some(
          ([offType, offListener, offOptions]) =>
            offType === type && offListener === listener && offOptions === options,
        ),
    );
    expect(left.map(([type]) => type)).toEqual([]);
    expect(added.mock.calls.map(([type]) => type)).toEqual(
      expect.arrayContaining(["pointermove", "pointerup", "pointercancel", "blur", "keydown"]),
    );
  });
});

describe("a drop", () => {
  it("between two tabs of another region: a tab there; the same view, said, the keyboard on its options", async () => {
    const host = await show();
    const field = host.querySelector<HTMLInputElement>(".probe-field");
    if (!field) throw new Error("no probe body");
    field.value = "typed";
    // by the header of a view alone in its group
    const title = titleOf(PROBE);
    drag(title, [800, 50], [69, 60]);
    expect(viewDrag.dragged).toBe(PROBE);
    await release(title, [69, 60]);
    expect(placed()).toEqual({ left: [[FILES, PROBE, SEARCH]], right: [[OTHER]], bottom: [] });
    expect(arrangement.isSelected(PROBE)).toBe(true);
    // never created again
    expect(hostOf(PROBE)).toBe(host);
    expect(probe.mounts).toBe(1);
    expect(field.value).toBe("typed");
    expect(said()).toBe("Outline moved to the sidebar");
    const [button] = stripButtons("sidebar");
    expect(button?.getAttribute("data-view-menu")).toBe(PROBE);
    expect(document.activeElement).toBe(button);
  });

  it("on the lower half of a view of another region: a group under it", async () => {
    await show();
    const search = tabOf(SEARCH);
    drag(search, [100, 60], [800, 600]);
    await release(search, [800, 600]);
    expect(placed()).toEqual({ left: [[FILES]], right: [[PROBE], [OTHER], [SEARCH]], bottom: [] });
    expect(arrangement.isSelected(SEARCH)).toBe(true);
    expect(said()).toBe("Search moved to the right panel");
    // alone in its group without a header of its own: its strip's options
    const [button] = stripButtons("right");
    expect(button?.getAttribute("data-view-menu")).toBe(SEARCH);
    expect(document.activeElement).toBe(button);
  });

  it("on the edge of a region that is not drawn: its first group; a region the person hid shows again", async () => {
    await show();
    layout.setBottomHidden(true);
    const search = tabOf(SEARCH);
    drag(search, [100, 60], [500, 660]);
    await release(search, [500, 660]);
    expect(placed()).toEqual({ left: [[FILES]], right: [[PROBE], [OTHER]], bottom: [[SEARCH]] });
    expect(layout.bottomHidden).toBe(false);
    expect(said()).toBe("Search moved to the bottom panel");
    expect(document.activeElement).toBe(stripButtons("bottom")[0]);
  });

  it("on the edge of a region the person hid (no room on screen): it shows again with the view as its last tab", async () => {
    await show();
    layout.setRightHidden(true);
    // the right region's views have no box on screen
    lay(
      START.filter(([is]) => is !== `[data-view-slot="${PROBE}"]` && !String(is).includes(OTHER)),
    );
    const search = tabOf(SEARCH);
    drag(search, [100, 60], [990, 300]);
    expect(targets().map((el) => el.getAttribute("data-drag-target"))).toEqual(["region"]);
    expect(targets()[0]?.textContent?.trim()).toBe("Move to right panel");
    await release(search, [990, 300]);
    expect(placed()).toEqual({ left: [[FILES]], right: [[PROBE], [OTHER, SEARCH]], bottom: [] });
    expect(layout.rightHidden).toBe(false);
    expect(said()).toBe("Search moved to the right panel");
  });

  it("inside its own region: under the view below it; into the strip beside it as a tab", async () => {
    await show();
    arrange([[FILES], [SEARCH]], [[PROBE], [OTHER]]);
    lay([
      ...FRAME,
      [stripWith(FILES), [0, 42, 260, 80]],
      [stripWith(SEARCH), [0, 363, 260, 401]],
      [`[role="tab"][data-view="${FILES}"]`, [8, 50, 68, 80]],
      [`[role="tab"][data-view="${SEARCH}"]`, [8, 371, 78, 401]],
      [`[data-view-slot="${FILES}"]`, [0, 80, 260, 356]],
      [`[data-view-slot="${SEARCH}"]`, [0, 401, 260, 676]],
      [`[data-view-slot="${PROBE}"]`, [720, 42, 1000, 356]],
      [`[data-view-slot="${OTHER}"]`, [720, 363, 1000, 676]],
    ]);
    const title = titleOf(PROBE);
    drag(title, [800, 50], [800, 600]);
    await release(title, [800, 600]);
    expect(placed().right).toEqual([[OTHER], [PROBE]]);
    expect(said()).toBe("Outline moved below");
    expect(document.activeElement).toBe(headerButton(PROBE));

    const files = tabOf(FILES);
    drag(files, [30, 60], [200, 380]);
    await release(files, [200, 380]);
    expect(placed().left).toEqual([[SEARCH, FILES]]);
    expect(arrangement.isSelected(FILES)).toBe(true);
    expect(said()).toBe("Files is now a tab");
    expect(document.activeElement).toBe(stripButtons("sidebar")[0]);
  });
});

describe("a drag that ends without a drop", () => {
  const ways: Record<string, (handle: HTMLElement) => void> = {
    "Escape pressed": () => {
      document.body.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
      );
    },
    "the window loses the focus": () => window.dispatchEvent(new Event("blur")),
    "the system takes the pointer": (handle) => pointer("pointercancel", handle, 800, 300),
    "the handle loses the pointer": (handle) => pointer("lostpointercapture", handle, 800, 300),
    "let go over the page": (handle) => pointer("pointerup", handle, 500, 300),
    "let go over its own tab": (handle) => pointer("pointerup", handle, 100, 60),
  };

  for (const [way, end] of Object.entries(ways)) {
    it(`${way}: nothing changed, the keyboard back where it was`, async () => {
      const host = await show();
      const field = host.querySelector<HTMLInputElement>(".probe-field");
      field?.focus();
      const before = arrangement.saved;
      const search = tabOf(SEARCH);
      pointer("pointerdown", search, 100, 60);
      // what a webview does with a press on a button
      search.focus();
      pointer("pointermove", search, 800, 300);
      expect(targets()).toHaveLength(1);
      end(search);
      flushSync();
      await tick();
      expect(viewDrag.state.status).toBe("idle");
      expect(layer()).toBeNull();
      expect(root.hasAttribute("data-view-drag")).toBe(false);
      expect(arrangement.saved).toBe(before);
      expect(said()).toBe("");
      expect(document.activeElement).toBe(field);
    });
  }

  it("Escape is taken before anything else hears it; other keys are not, nor Escape while text is composed", async () => {
    await show();
    const heard = vi.fn();
    window.addEventListener("keydown", heard);
    try {
      drag(tabOf(SEARCH), [100, 60], [800, 300]);
      const press = (init: KeyboardEventInit): KeyboardEvent => {
        const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
        document.body.dispatchEvent(event);
        return event;
      };
      expect(press({ key: "a" }).defaultPrevented).toBe(false);
      expect(press({ key: "Escape", isComposing: true }).defaultPrevented).toBe(false);
      expect(heard).toHaveBeenCalledTimes(2);
      expect(viewDrag.state.status).toBe("dragging");
      expect(press({ key: "Escape" }).defaultPrevented).toBe(true);
      expect(heard).toHaveBeenCalledTimes(2);
      expect(viewDrag.state.status).toBe("idle");
      // no drag runs: Escape is anybody's again
      expect(press({ key: "Escape" }).defaultPrevented).toBe(false);
      expect(heard).toHaveBeenCalledTimes(3);
    } finally {
      window.removeEventListener("keydown", heard);
    }
  });
});

describe("the click a drag ends with", () => {
  it("is not a click on the tab; the next one is", async () => {
    await show();
    const search = tabOf(SEARCH);
    drag(search, [100, 60], [500, 300]);
    pointer("pointerup", search, 500, 300);
    // the webview's click on the handle that held the pointer
    mouseClick(search);
    expect(arrangement.isSelected(SEARCH)).toBe(false);
    await later();
    mouseClick(search);
    expect(arrangement.isSelected(SEARCH)).toBe(true);
  });

  it("is not one either when the drag ended by Escape and the button is let go later", async () => {
    await show();
    const search = tabOf(SEARCH);
    drag(search, [100, 60], [500, 300]);
    viewDrag.cancel();
    flushSync();
    await later();
    pointer("pointerup", search, 100, 60);
    mouseClick(search);
    expect(arrangement.isSelected(SEARCH)).toBe(false);
    await later();
    // a new press is a new story
    pointer("pointerdown", search, 100, 60);
    pointer("pointerup", search, 100, 60);
    mouseClick(search);
    expect(arrangement.isSelected(SEARCH)).toBe(true);
  });

  it("the Escape that ends a drag is not the key that ends the wait: the release after it is still no click", async () => {
    await show();
    const search = tabOf(SEARCH);
    drag(search, [100, 60], [500, 300]);
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    flushSync();
    expect(viewDrag.state.status).toBe("idle");
    pointer("pointerup", search, 100, 60);
    mouseClick(search);
    expect(arrangement.isSelected(SEARCH)).toBe(false);
  });

  const lost: Record<string, (handle: HTMLElement) => void> = {
    "the window lost the focus": () => window.dispatchEvent(new Event("blur")),
    "the system took the pointer": (handle) => pointer("pointercancel", handle, 500, 300),
  };
  for (const [way, end] of Object.entries(lost)) {
    it(`${way} with the button still down, its release never seen: a key's click on a button is that button's`, async () => {
      await show();
      const search = tabOf(SEARCH);
      drag(search, [100, 60], [500, 300]);
      end(search);
      flushSync();
      expect(viewDrag.state.status).toBe("idle");
      // Enter | Space on a button: a click no pointer made (`detail` 0)
      search.click();
      flushSync();
      expect(arrangement.isSelected(SEARCH)).toBe(true);
    });
  }

  it("the first key pressed ends the wait for a release that never came: the next click with the pointer is a click", async () => {
    await show();
    const search = tabOf(SEARCH);
    drag(search, [100, 60], [500, 300]);
    window.dispatchEvent(new Event("blur"));
    flushSync();
    document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true }));
    mouseClick(search);
    expect(arrangement.isSelected(SEARCH)).toBe(true);
  });
});

describe("a press whose release the window never saw", () => {
  it("is over at the next move with no button down: no drag starts from it", async () => {
    await show();
    const search = tabOf(SEARCH);
    pointer("pointerdown", search, 100, 60);
    expect(viewDrag.state.status).toBe("pending");
    // the pointer comes back over the window, no button held
    pointer("pointermove", search, 800, 300, { buttons: 0 });
    expect(viewDrag.state.status).toBe("idle");
    expect(layer()).toBeNull();
    // the press is over for good: a later move with a button down is not its drag
    pointer("pointermove", search, 800, 310);
    expect(viewDrag.state.status).toBe("idle");
  });

  it("a drag that runs ends the same way: nothing changed, nothing of it left", async () => {
    await show();
    const search = tabOf(SEARCH);
    const before = arrangement.saved;
    drag(search, [100, 60], [800, 300]);
    expect(viewDrag.dragged).toBe(SEARCH);
    pointer("pointermove", search, 800, 600, { buttons: 0 });
    expect(viewDrag.state.status).toBe("idle");
    expect(layer()).toBeNull();
    expect(arrangement.saved).toBe(before);
    expect(said()).toBe("");
  });
});

describe("a drop into a bottom region the window is too short to show", () => {
  it("makes the move and says how to see the view", async () => {
    await show();
    layout.setCenterHeight(PAGE_MIN_H + BOTTOM.min - 1);
    const search = tabOf(SEARCH);
    drag(search, [100, 60], [500, 660]);
    expect(targets()[0]?.textContent?.trim()).toBe("Move to bottom panel");
    await release(search, [500, 660]);
    expect(placed().bottom).toEqual([[SEARCH]]);
    expect(said()).toBe("Search moved to the bottom panel. Make the window taller to show it.");
  });
});

describe("what does not start a drag", () => {
  it("the options button in a view's header, the one at the end of a strip, a control of the view", async () => {
    const host = await show();
    const inHeader = headerButton(PROBE);
    const [atStrip] = stripButtons("sidebar");
    const field = host.querySelector<HTMLElement>(".probe-field");
    for (const el of [inHeader, atStrip, field]) {
      if (!el) throw new Error("no control");
      drag(el, [100, 60], [500, 300]);
      expect(viewDrag.state.status).toBe("idle");
      pointer("pointerup", el, 500, 300);
    }
    expect(layer()).toBeNull();
  });

  it("the header of a view that is a tab of a group: its tab is the handle", async () => {
    await show();
    arrange([[FILES, SEARCH]], [[PROBE, OTHER]]);
    drag(titleOf(PROBE), [800, 90], [500, 300]);
    expect(viewDrag.state.status).toBe("idle");
    drag(tabOf(PROBE), [800, 60], [500, 300]);
    expect(viewDrag.dragged).toBe(PROBE);
  });
});

describe("the look of a drag", () => {
  const css = readFileSync(resolve(process.cwd(), "src/core/shell/ViewDragLayer.svelte"), "utf8");
  const style = css.slice(css.indexOf("<style>"));

  it("never animates (nothing to turn off for reduced motion) and takes no pointer", () => {
    expect(style).not.toMatch(/transition|animation/);
    expect(style).toMatch(/\.layer \{[^}]*pointer-events: none;/);
  });

  it("uses the theme's tokens only: the accent for the place to drop", () => {
    expect(style).not.toMatch(/#[0-9a-f]{3,8}\b|rgba?\(|hsla?\(/i);
    expect(style).toMatch(/\.tab \{[^}]*background: var\(--accent\);/);
    expect(style).toMatch(/border: 2px solid var\(--accent\);/);
  });

  it("while it runs the page selects no text, and the cursor says whether a drop here does something", () => {
    expect(style).toMatch(/:global\(html\[data-view-drag\]\) \{[^}]*user-select: none;/);
    expect(style).toMatch(/:global\(html\[data-view-drag="none"\] \*\) \{[^}]*cursor: not-allowed/);
    expect(style).toMatch(/:global\(html\[data-view-drag="target"\] \*\) \{[^}]*cursor: grabbing/);
  });
});
