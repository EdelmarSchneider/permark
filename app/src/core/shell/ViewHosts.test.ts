// @vitest-environment jsdom
// The views of the window are created once and moved: a view that changes group | region keeps its component (what
// was typed, how far a list was scrolled); a region draws tab strips, slots and dividers, never a view's body.
import { flushSync, mount, unmount } from "svelte";
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

const { createMockApi } = await import("../../api/mock");
const { NO_REQUIREMENT } = await import("../conditions/available");
const { conditions, neutralContext } = await import("../conditions/context.svelte");
const { contributions } = await import("../plugins/instance.svelte");
const { default: RegionsHost } = await import("../testing/RegionsHost.svelte");
const { probe } = await import("../testing/ViewProbe.svelte");
const { layout } = await import("./layout.svelte");
const { arrangement } = await import("./arrangement.svelte");
const { DIVIDER_PX, FILES, MIN_GROUP_PX, REGIONS, SEARCH } = await import("./arrangement");
const { selectView } = await import("./sides");

/** A right panel with a header of its own, a field and a list (the probe); a second right panel; a sidebar tab. */
const PROBE = "probe.panel";
const OTHER = "other.panel";
const LEFT = "left.panel";

let api: Api;
let component: Record<string, unknown> | null = null;
let stop: () => void = () => {};
const removals: (() => void)[] = [];

function panel(id: string, side: "left" | "right", view: () => Promise<unknown>): void {
  removals.push(
    contributions.register({
      kind: "panel",
      id,
      owner: "test" as PluginId,
      side,
      label: side === "left" ? "unread.title" : "live.label",
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

const slot = (id: string): HTMLElement | null =>
  document.querySelector<HTMLElement>(`[data-view-slot="${id}"]`);
const hostsOf = (id: string): HTMLElement[] => [
  ...document.querySelectorAll<HTMLElement>(`[data-view-host="${id}"]`),
];
const regionAt = (el: Element | null): string | null =>
  el?.closest("[data-region]")?.getAttribute("data-region") ?? null;
const strips = (region: string): string[][] =>
  [...document.querySelectorAll(`[data-region="${region}"] [role="tablist"]`)].map((strip) =>
    [...strip.querySelectorAll('[role="tab"]')].map((tab) => tab.getAttribute("title") ?? ""),
  );

/** The window's regions on screen, the probe's view loaded. */
async function show(): Promise<HTMLElement> {
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(RegionsHost, { target });
  flushSync();
  await vi.waitFor(() => expect(slot(PROBE)?.querySelector("[data-probe]")).toBeTruthy());
  const [host] = hostsOf(PROBE);
  if (host === undefined) throw new Error("no probe view");
  return host;
}

beforeEach(async () => {
  document.body.replaceChildren();
  probe.mounts = 0;
  api = createMockApi();
  panel(PROBE, "right", () => import("../testing/ViewProbe.svelte"));
  panel(OTHER, "right", () => import("../testing/StripFirst.svelte"));
  stop = arrangement.start(api);
  conditions.set({ ...neutralContext(), workspace: "project" });
  await api.projectSessionSet("layout.views", null);
  await layout.load(api, true);
  layout.setOpen(PROBE, true);
  layout.setOpen(OTHER, true);
  arrange([[FILES, SEARCH]], [[PROBE], [OTHER]]);
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  stop();
  for (const remove of removals.splice(0)) remove();
  conditions.set(neutralContext());
  for (const region of REGIONS) arrangement.setSize(region, 0);
  await layout.load(api, false);
  vi.restoreAllMocks();
});

describe("a view that moves", () => {
  it("to another group, then to the other region, is the same component: what was typed and scrolled stays", async () => {
    const host = await show();
    const field = host.querySelector<HTMLInputElement>(".probe-field");
    const list = host.querySelector<HTMLElement>(".probe-list");
    if (!field || !list) throw new Error("no probe body");
    field.value = "typed";
    list.scrollTop = 37;
    const same = (region: string, what: string): void => {
      expect(hostsOf(PROBE), what).toEqual([host]);
      expect(slot(PROBE)?.contains(host), `${what}: in its slot`).toBe(true);
      expect(regionAt(host), what).toBe(region);
      expect(probe.mounts, `${what}: created once`).toBe(1);
      expect(host.querySelector(".probe-field"), what).toBe(field);
      expect(field.value, what).toBe("typed");
      expect(list.scrollTop, what).toBe(37);
    };
    same("right", "at the start");

    arrange([[FILES, SEARCH]], [[OTHER, PROBE]]);
    same("right", "a tab of the other group");
    arrange([[FILES, SEARCH]], [[OTHER], [PROBE]]);
    same("right", "a group of its own again, under the other");
    arrange([[FILES, SEARCH, PROBE]], [[OTHER]]);
    same("sidebar", "a tab of the left region");
    arrange([[PROBE], [FILES, SEARCH]], [[OTHER]]);
    same("sidebar", "a group of its own at the left");
    arrange([[FILES, SEARCH]], [[OTHER]], [[PROBE]]);
    same("bottom", "alone at the bottom");
    arrange([[FILES]], [], [[SEARCH], [OTHER, PROBE]]);
    same("bottom", "a tab of a group beside another at the bottom");
    arrange([[FILES, SEARCH]], [[PROBE], [OTHER]]);
    same("right", "back at the right");
  });

  it("gets its scroll offsets back where a browser forgets them (an element taken out of the page and put back)", async () => {
    const host = await show();
    const list = host.querySelector<HTMLElement>(".probe-list");
    if (!list) throw new Error("no probe body");
    // what a browser does and jsdom does not: scroll offsets are lost with every move in the page
    const forget = (node: unknown): void => {
      if (!(node instanceof Element) || !node.isConnected) return;
      for (const el of [node, ...node.querySelectorAll("*")]) el.scrollTop = 0;
    };
    const elementBefore = Element.prototype.before;
    const textBefore = CharacterData.prototype.before;
    const remove = Element.prototype.remove;
    const append = Node.prototype.appendChild;
    vi.spyOn(Element.prototype, "before").mockImplementation(function (this: Element, ...nodes) {
      nodes.forEach(forget);
      elementBefore.apply(this, nodes);
    });
    vi.spyOn(CharacterData.prototype, "before").mockImplementation(function (
      this: CharacterData,
      ...nodes
    ) {
      nodes.forEach(forget);
      textBefore.apply(this, nodes);
    });
    vi.spyOn(Element.prototype, "remove").mockImplementation(function (this: Element) {
      forget(this);
      remove.call(this);
    });
    vi.spyOn(Node.prototype, "appendChild").mockImplementation(function <T extends Node>(
      this: Node,
      node: T,
    ): T {
      forget(node);
      return append.call(this, node) as T;
    });

    list.scrollTop = 37;
    // inside the region: the slot itself changes place
    arrange([[FILES, SEARCH]], [[OTHER], [PROBE]]);
    expect(list.scrollTop, "moved under the other group").toBe(37);
    // to the other region: the old slot goes, the view moves into a new one
    arrange([[PROBE], [FILES, SEARCH]], [[OTHER]]);
    expect(regionAt(host)).toBe("sidebar");
    expect(list.scrollTop, "moved to the left region").toBe(37);
  });

  it("is moved with `moveBefore` where the webview has it (focus and running animations survive)", async () => {
    const host = await show();
    const moved: Node[] = [];
    const moveBefore = function (this: Element, node: Node, child: Node | null): void {
      if (!node.isConnected || !this.isConnected) throw new Error("not in the page");
      moved.push(node);
      this.insertBefore(node, child);
    };
    Object.defineProperty(Element.prototype, "moveBefore", {
      value: moveBefore,
      configurable: true,
    });
    const appended = vi.spyOn(Node.prototype, "appendChild");
    try {
      arrange([[FILES, SEARCH, PROBE]], [[OTHER]]);
      expect(regionAt(host)).toBe("sidebar");
      expect(slot(PROBE)?.contains(host)).toBe(true);
      // never out of the page on its way (it left its old slot before the region took that slot away): every step
      // was a move of a connected element, none an insertion
      expect(moved.length).toBeGreaterThan(0);
      expect(moved.every((node) => node === host)).toBe(true);
      expect(appended.mock.calls.some(([node]) => node === host)).toBe(false);
    } finally {
      Reflect.deleteProperty(Element.prototype, "moveBefore");
    }
  });

  it("is removed, element and all, when it stops being present; opened again it is a new one", async () => {
    const host = await show();
    layout.setOpen(PROBE, false);
    flushSync();
    expect(hostsOf(PROBE)).toEqual([]);
    expect(slot(PROBE)).toBeNull();
    expect(host.isConnected).toBe(false);
    expect(document.querySelector("[data-probe]")).toBeNull();
    layout.setOpen(PROBE, true);
    flushSync();
    await vi.waitFor(() => expect(slot(PROBE)?.querySelector("[data-probe]")).toBeTruthy());
    expect(probe.mounts).toBe(2);
    expect(hostsOf(PROBE)).toHaveLength(1);
  });
});

describe("a region", () => {
  it("the default left region: one strip Files | Search | the plugins' tabs, one view shown, the others inert in place", async () => {
    panel(LEFT, "left", () => import("../testing/StripSecond.svelte"));
    arrangement.arrange({ ...arrangement.saved, left: [{ views: [FILES, SEARCH], share: 1 }] });
    await show();
    expect(strips("sidebar")).toEqual([["Files", "Search", "Unread"]]);
    expect(
      document
        .querySelector('[data-region="sidebar"] [role="tablist"]')
        ?.getAttribute("aria-label"),
    ).toBe("Sidebar views");
    const shown = (): boolean[] => [FILES, SEARCH, LEFT].map((id) => slot(id)?.inert !== true);
    expect(shown()).toEqual([true, false, false]);
    // a tab behind another is never taken out of the layout (its lists keep their place)
    for (const id of [FILES, SEARCH, LEFT]) {
      expect(slot(id)?.hidden, id).toBe(false);
      expect(slot(id)?.id, id).toBe(`view-${id}`);
    }
    expect(slot(SEARCH)?.classList.contains("off")).toBe(true);
    selectView(SEARCH);
    flushSync();
    expect(shown()).toEqual([false, true, false]);
    document.querySelector<HTMLElement>(`[role="tab"][data-view="${LEFT}"]`)?.click();
    flushSync();
    expect(shown()).toEqual([false, false, true]);
  });

  it("a view without a header of its own keeps a strip when alone in its group; one with its own header has none", async () => {
    await show();
    // right: two groups of one view each, both with their own header -> no strip (as the right column was)
    expect(strips("right")).toEqual([]);
    arrange([[FILES], [SEARCH]], [[PROBE], [OTHER]]);
    expect(strips("sidebar")).toEqual([["Files"], ["Search"]]);
    expect([FILES, SEARCH].map((id) => slot(id)?.inert !== true)).toEqual([true, true]);
    // the probe alone among sidebar views: still no strip of its own
    arrange([[FILES, SEARCH], [PROBE]], [[OTHER]]);
    expect(strips("sidebar")).toEqual([["Files", "Search"]]);
    // tabs at the right: one strip, named for the right panel
    arrange([[FILES, SEARCH]], [[PROBE, OTHER]]);
    expect(strips("right")).toEqual([["Live changes", "Live changes"]]);
    expect(
      document.querySelector('[data-region="right"] [role="tablist"]')?.getAttribute("aria-label"),
    ).toBe("Right panel views");
  });

  it("one options menu for the window: a tab's right click and a view's button open the same one", async () => {
    const host = await show();
    const menus = (): Element[] => [...document.querySelectorAll('[role="menu"]')];
    const items = (): string[] =>
      [...document.querySelectorAll('[role="menu"] [role="menuitem"]')].map(
        (item) => item.textContent?.trim() ?? "",
      );
    host.querySelector<HTMLElement>(`[data-view-menu="${PROBE}"]`)?.click();
    flushSync();
    expect(menus()).toHaveLength(1);
    expect(items()).toEqual([
      "Move up",
      "Move down",
      "Show as tab",
      "Move to left",
      "Move to bottom",
      "Close",
      "Reset layout",
    ]);
    // a sidebar tab: the same menu, for that view; it cannot be closed
    document
      .querySelector(`[role="tab"][data-view="${SEARCH}"]`)
      ?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    flushSync();
    expect(menus()).toHaveLength(1);
    expect(items()).toEqual([
      "Show above",
      "Show below",
      "Move to right",
      "Move to bottom",
      "Reset layout",
    ]);
  });

  it("at the bottom puts its groups side by side: a column each, the strip over its group, an upright divider between", async () => {
    await show();
    arrange([[FILES]], [[OTHER]], [[SEARCH], [PROBE]]);
    const region = document.querySelector<HTMLElement>('[data-view-region="bottom"]');
    if (region === null) throw new Error("no bottom region");
    const at = (el: Element | null | undefined): string[] => {
      const style = el instanceof HTMLElement ? el.style : null;
      return [
        style?.getPropertyValue("grid-column") ?? "",
        style?.getPropertyValue("grid-row") ?? "",
      ];
    };
    // the groups' shares are widths; the rows are the strips' and the contents' (the stylesheet's)
    expect(region.style.getPropertyValue("grid-template-columns")).toBe(
      `minmax(0, 50.000fr) ${DIVIDER_PX}px minmax(0, 50.000fr)`,
    );
    expect(region.style.getPropertyValue("grid-template-rows")).toBe("");
    // Search has no header of its own: its strip sits over it, in its column
    const strip = region.querySelector('[role="tablist"]');
    expect(strips("bottom")).toEqual([["Search"]]);
    expect(strip?.getAttribute("aria-label")).toBe("Bottom panel views");
    expect(at(strip?.closest(".strip"))).toEqual(["1", "1"]);
    expect(at(slot(SEARCH))).toEqual(["1", "2"]);
    // the divider and a view with its own header take the whole height
    const bar = region.querySelector('[role="separator"]');
    expect(bar?.getAttribute("aria-orientation")).toBe("vertical");
    expect(at(bar?.parentElement)).toEqual(["2", "1 / -1"]);
    expect(at(slot(PROBE))).toEqual(["3", "1 / -1"]);
    // seen from left to right = the Tab order
    expect(
      [...region.children].map(
        (el) =>
          el.getAttribute("data-view-slot") ?? el.querySelector("[role]")?.getAttribute("role"),
      ),
    ).toEqual(["tablist", SEARCH, "separator", PROBE]);
    // a side region still stacks: rows, one column
    const right = document.querySelector<HTMLElement>('[data-view-region="right"]');
    expect(right?.style.getPropertyValue("grid-template-columns")).toBe("");
    expect(right?.style.getPropertyValue("grid-template-rows")).not.toBe("");
  });

  it("at the bottom its divider moves the widths: the shares are saved", async () => {
    await show();
    arrange([[FILES, SEARCH]], [], [[PROBE], [OTHER]]);
    const bar = document.querySelector<HTMLElement>('[data-region="bottom"] [role="separator"]');
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
      width: 400,
      height: 240,
    } as DOMRect);
    bar?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
    flushSync();
    expect(arrangement.saved.bottom.map((group) => group.share)).toEqual([0.52, 0.48]);
    expect(arrangement.saved.bottom.map((group) => group.views)).toEqual([[PROBE], [OTHER]]);
  });

  it("at the bottom too narrow for its groups (210 px each) shows them as one tab group; as arranged again when wide", async () => {
    await show();
    arrange([[FILES]], [], [[SEARCH], [PROBE], [OTHER]]);
    const need = 3 * MIN_GROUP_PX + 2 * DIVIDER_PX;
    const separators = (): number =>
      document.querySelectorAll('[data-region="bottom"] [role="separator"]').length;
    expect(separators()).toBe(2);
    arrangement.setSize("bottom", need - 1);
    flushSync();
    expect(strips("bottom")).toEqual([["Search", "Live changes", "Live changes"]]);
    expect(separators()).toBe(0);
    expect(
      document
        .querySelector<HTMLElement>('[data-view-region="bottom"]')
        ?.style.getPropertyValue("grid-template-columns"),
    ).toBe("minmax(0, 100.000fr)");
    // what the person arranged is untouched
    expect(arrangement.saved.bottom.map((group) => group.views)).toEqual([
      [SEARCH],
      [PROBE],
      [OTHER],
    ]);
    arrangement.setSize("bottom", need);
    flushSync();
    expect(strips("bottom")).toEqual([["Search"]]);
    expect(separators()).toBe(2);
  });

  it("measures the room along its own stack: the bottom its width, a side region its height", async () => {
    const wide = vi.spyOn(Element.prototype, "clientWidth", "get").mockReturnValue(400);
    const high = vi.spyOn(Element.prototype, "clientHeight", "get").mockReturnValue(900);
    arrange([[FILES], [SEARCH]], [], [[PROBE], [OTHER]]);
    await show();
    // 400 px wide: no room for two groups side by side; 900 px high: room for two, one above the other
    expect(strips("bottom")).toEqual([["Live changes", "Live changes"]]);
    expect(strips("sidebar")).toEqual([["Files"], ["Search"]]);
    await unmount(component as Record<string, unknown>);
    component = null;
    for (const region of REGIONS) arrangement.setSize(region, 0);
    wide.mockReturnValue(900);
    high.mockReturnValue(400);
    await show();
    expect(strips("bottom")).toEqual([]);
    expect(strips("sidebar")).toEqual([["Files", "Search"]]);
  });

  it("says what a move did in one place for the whole window", async () => {
    await show();
    const said = (): Element[] => [...document.querySelectorAll("[data-view-status]")];
    expect(said()).toHaveLength(1);
    expect(said()[0]?.getAttribute("role")).toBe("status");
    arrangement.status = "Outline moved down";
    flushSync();
    expect(said()[0]?.textContent).toBe("Outline moved down");
  });
});
