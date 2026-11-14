// @vitest-environment jsdom
// The options menu of a view over the three regions: its entries by where the view sits (alone | a tab; at a side | at
// the bottom), the moves to another region, Close, the reset of the whole arrangement; where the options button shows;
// what follows an action (the keyboard on the view's options, the move said).
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

const { createMockApi } = await import("../../api/mock");
const { NO_REQUIREMENT } = await import("../conditions/available");
const { conditions, neutralContext } = await import("../conditions/context.svelte");
const { contributions } = await import("../plugins/instance.svelte");
const { default: RegionsHost } = await import("../testing/RegionsHost.svelte");
const { probe } = await import("../testing/ViewProbe.svelte");
const { layout } = await import("./layout.svelte");
const { arrangement } = await import("./arrangement.svelte");
const { defaultArrangement, DIVIDER_PX, FILES, MIN_GROUP_PX, OUTLINE, REGIONS, SEARCH } =
  await import("./arrangement");
const { selectView } = await import("./sides");
const { viewMenu } = await import("./viewMenu.svelte");

/** A right panel with a header of its own (the probe), a second right panel without one, a sidebar tab. */
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
      label: side === "left" ? "unread.title" : id === PROBE ? "outline.tab" : "live.label",
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

/** Where the views of these tests sit now, region by region (the outline, known to every window and not present
 *  here, is left out). */
const placed = (): Record<string, string[][]> =>
  Object.fromEntries(
    REGIONS.map((region) => [
      region,
      arrangement.saved[region]
        .map((g) => g.views.filter((id) => id !== OUTLINE))
        .filter((views) => views.length > 0),
    ]),
  );

/** The entries of a view's menu, as a person reads them: `—` between blocks, `#` a heading, `(off)` = cannot act. */
const menuOf = (id: string): string[] =>
  viewMenu.entries(id).map((entry) => {
    if (entry.kind === "separator") return "—";
    if (entry.kind === "heading") return `# ${entry.label}`;
    return entry.disabled === true ? `${entry.label} (off)` : entry.label;
  });

const slot = (id: string): HTMLElement | null =>
  document.querySelector<HTMLElement>(`[data-view-slot="${id}"]`);
const hostOf = (id: string): HTMLElement | null =>
  document.querySelector<HTMLElement>(`[data-view-host="${id}"]`);
const regionAt = (el: Element | null): string | null =>
  el?.closest("[data-region]")?.getAttribute("data-region") ?? null;
/** The options button a view draws in its own header. */
const headerButton = (id: string): HTMLElement | null =>
  hostOf(id)?.querySelector<HTMLElement>(`[data-view-menu="${id}"]`) ?? null;
/** The options buttons at the end of the tab strips of a region. */
const stripButtons = (region: string): HTMLElement[] =>
  [...document.querySelectorAll<HTMLElement>(`[data-region="${region}"] [data-view-menu]`)].filter(
    (button) => button.closest("[data-view-host]") === null,
  );
const said = (): string => document.querySelector("[data-view-status]")?.textContent ?? "";
const menuItems = (): HTMLElement[] => [
  ...document.querySelectorAll<HTMLElement>('[role="menu"] [role="menuitem"]'),
];

/** Picks an entry of the open menu; the move is drawn and the keyboard has settled. */
async function pick(label: string): Promise<void> {
  const item = menuItems().find((el) => el.textContent?.trim() === label);
  if (item === undefined) throw new Error(`no entry "${label}"`);
  item.click();
  flushSync();
  await tick();
  await tick();
}

/** Opens the menu from an options button. */
function press(button: HTMLElement | null | undefined): void {
  if (!button) throw new Error("no options button");
  button.focus();
  button.click();
  flushSync();
}

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
  viewMenu.close();
  if (component) await unmount(component);
  component = null;
  stop();
  for (const remove of removals.splice(0)) remove();
  conditions.set(neutralContext());
  for (const region of REGIONS) arrangement.setSize(region, 0);
  arrangement.status = "";
  await layout.load(api, false);
  vi.restoreAllMocks();
});

describe("the entries of a view's menu", () => {
  it("at the right, alone in its group: move up | down, show as tab; the other regions; Close; Reset", () => {
    expect(menuOf(PROBE)).toEqual([
      "Move up (off)",
      "Move down",
      "Show as tab",
      "—",
      "Move to left",
      "Move to bottom",
      "—",
      "Close",
      "—",
      "Reset layout",
    ]);
    // the last drawn group cannot go further down
    expect(menuOf(OTHER).slice(0, 3)).toEqual(["Move up", "Move down (off)", "Show as tab"]);
  });

  it("at the right, a tab of a group: show above | below", () => {
    arrange([[FILES, SEARCH]], [[PROBE, OTHER]]);
    expect(menuOf(PROBE)).toEqual([
      "Show above",
      "Show below",
      "—",
      "Move to left",
      "Move to bottom",
      "—",
      "Close",
      "—",
      "Reset layout",
    ]);
  });

  it("at the left: the same words; Files, Search and a plugin's sidebar tab have no Close", () => {
    panel(LEFT, "left", () => import("../testing/StripSecond.svelte"));
    arrange([[FILES, SEARCH, LEFT]], [[PROBE], [OTHER]]);
    for (const id of [FILES, SEARCH, LEFT]) {
      expect(menuOf(id), id).toEqual([
        "Show above",
        "Show below",
        "—",
        "Move to right",
        "Move to bottom",
        "—",
        "Reset layout",
      ]);
    }
    arrange([[FILES], [SEARCH, LEFT]], [[PROBE], [OTHER]]);
    expect(menuOf(FILES).slice(0, 4)).toEqual(["Move up (off)", "Move down", "Show as tab", "—"]);
  });

  it("at the bottom the groups sit side by side: move left | right, show on the left | on the right", () => {
    arrange([[FILES]], [], [[SEARCH], [PROBE, OTHER]]);
    expect(menuOf(SEARCH)).toEqual([
      "Move left (off)",
      "Move right",
      "Show as tab",
      "—",
      "Move to left",
      "Move to right",
      "—",
      "Reset layout",
    ]);
    expect(menuOf(PROBE)).toEqual([
      "Show on the left",
      "Show on the right",
      "—",
      "Move to left",
      "Move to right",
      "—",
      "Close",
      "—",
      "Reset layout",
    ]);
  });

  it("a view alone in its region has nothing to arrange there: only the other regions, Close, Reset", () => {
    layout.setOpen(OTHER, false);
    expect(menuOf(PROBE)).toEqual([
      "Move to left",
      "Move to bottom",
      "—",
      "Close",
      "—",
      "Reset layout",
    ]);
    // the only view at the bottom
    layout.setOpen(OTHER, true);
    arrange([[FILES, SEARCH]], [[PROBE]], [[OTHER]]);
    expect(menuOf(OTHER).slice(0, 2)).toEqual(["Move to left", "Move to right"]);
  });

  it("a region with three groups takes no more: show above | below cannot act", () => {
    panel(LEFT, "left", () => import("../testing/StripSecond.svelte"));
    arrange([[FILES], [SEARCH], [LEFT, PROBE]], [[OTHER]]);
    expect(menuOf(PROBE).slice(0, 2)).toEqual(["Show above (off)", "Show below (off)"]);
    arrange([[FILES]], [], [[SEARCH], [LEFT], [PROBE, OTHER]]);
    expect(menuOf(PROBE).slice(0, 2)).toEqual([
      "Show on the left (off)",
      "Show on the right (off)",
    ]);
  });

  it("a region folded by a small window says why and arranges nothing: taller at a side, wider at the bottom", () => {
    arrangement.setSize("right", 2 * MIN_GROUP_PX + DIVIDER_PX - 1);
    expect(menuOf(PROBE).slice(0, 5)).toEqual([
      "# Make the window taller to show these views one above the other.",
      "Move up (off)",
      "Move down (off)",
      "Show as tab (off)",
      "—",
    ]);
    // the moves to another region still act
    expect(menuOf(PROBE).slice(5, 7)).toEqual(["Move to left", "Move to bottom"]);
    arrange([[FILES]], [], [[SEARCH], [PROBE, OTHER]]);
    arrangement.setSize("bottom", 2 * MIN_GROUP_PX + DIVIDER_PX - 1);
    expect(menuOf(SEARCH).slice(0, 4)).toEqual([
      "# Make the window wider to show these views side by side.",
      "Move left (off)",
      "Move right (off)",
      "Show as tab (off)",
    ]);
    expect(menuOf(PROBE).slice(0, 3)).toEqual([
      "# Make the window wider to show these views side by side.",
      "Show on the left (off)",
      "Show on the right (off)",
    ]);
  });

  it("Reset layout cannot act while the arrangement is the default one", () => {
    arrangement.arrange(defaultArrangement());
    expect(menuOf(SEARCH).at(-1)).toBe("Reset layout (off)");
    arrange([[SEARCH, FILES]], [[PROBE], [OTHER]]);
    expect(menuOf(SEARCH).at(-1)).toBe("Reset layout");
  });

  it("a view no region lists has no entries", () => {
    expect(menuOf("gone.panel")).toEqual([]);
  });
});

describe("the options button", () => {
  it("a tab strip has one at its end, for the tab it shows: quiet until the strip is pointed at | has the keyboard", async () => {
    await show();
    const [button, ...more] = stripButtons("sidebar");
    expect(more).toEqual([]);
    expect(button?.tagName).toBe("BUTTON");
    expect(button?.getAttribute("data-view-menu")).toBe(FILES);
    expect(button?.getAttribute("aria-label")).toBe("Files options");
    expect(button?.getAttribute("title")).toBe("Files options");
    expect(button?.getAttribute("aria-haspopup")).toBe("menu");
    expect(button?.getAttribute("aria-expanded")).toBe("false");
    // kept in the page and in the Tab order while it is not seen: after the tabs, outside their list
    expect(button?.classList.contains("options")).toBe(true);
    expect(button?.hidden).toBe(false);
    expect(button?.tabIndex).toBe(0);
    const strip = document.querySelector('[data-region="sidebar"] [role="tablist"]');
    expect(strip?.contains(button ?? null)).toBe(false);
    expect(
      strip && button && strip.compareDocumentPosition(button) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect([...(strip?.children ?? [])].every((el) => el.getAttribute("role") === "tab")).toBe(
      true,
    );
  });

  it("acts on the selected tab: its name, its menu; pressed again the menu closes", async () => {
    await show();
    selectView(SEARCH);
    flushSync();
    const [button] = stripButtons("sidebar");
    expect(button?.getAttribute("data-view-menu")).toBe(SEARCH);
    expect(button?.getAttribute("aria-label")).toBe("Search options");
    press(button);
    expect(viewMenu.open?.id).toBe(SEARCH);
    expect(document.querySelector('[role="menu"]')?.getAttribute("aria-label")).toBe(
      "Search options",
    );
    expect(button?.getAttribute("aria-expanded")).toBe("true");
    button?.click();
    flushSync();
    expect(document.querySelector('[role="menu"]')).toBeNull();
    expect(button?.getAttribute("aria-expanded")).toBe("false");
  });

  it("a view with its own header alone in its group carries it; as a tab of a group it has none (the strip has it)", async () => {
    await show();
    expect(headerButton(PROBE)).not.toBeNull();
    expect(stripButtons("right")).toEqual([]);
    arrange([[FILES, SEARCH]], [[PROBE, OTHER]]);
    expect(headerButton(PROBE)).toBeNull();
    const [button, ...more] = stripButtons("right");
    expect(more).toEqual([]);
    expect(button?.getAttribute("data-view-menu")).toBe(PROBE);
    selectView(OTHER);
    flushSync();
    expect(stripButtons("right")[0]?.getAttribute("data-view-menu")).toBe(OTHER);
    expect(document.querySelectorAll(`[data-view-menu="${PROBE}"]`)).toHaveLength(0);
  });

  it("follows the window, not the region: a view alone at the right still has it beside the sidebar's views", async () => {
    await show();
    layout.setOpen(OTHER, false);
    flushSync();
    expect(arrangement.presentIn("right")).toBe(1);
    expect(headerButton(PROBE)).not.toBeNull();
  });

  it("a window with one view shows none; a right click on the view's header still opens its menu", async () => {
    conditions.set({ ...neutralContext(), workspace: "singleFile" });
    layout.setOpen(OTHER, false);
    const host = await show();
    expect(arrangement.presentCount).toBe(1);
    expect(document.querySelectorAll("[data-view-menu]")).toHaveLength(0);
    host
      .querySelector("[data-view-header]")
      ?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    flushSync();
    expect(menuItems().map((item) => item.textContent?.trim())).toEqual([
      "Move to left",
      "Move to bottom",
      "Close",
      "Reset layout",
    ]);
    // Shift+F10 on something in the header opens it too
    viewMenu.close();
    flushSync();
    host
      .querySelector("[data-view-header] h2")
      ?.dispatchEvent(new KeyboardEvent("keydown", { key: "F10", shiftKey: true, bubbles: true }));
    flushSync();
    expect(viewMenu.open?.id).toBe(PROBE);
  });
});

describe("an action of the menu", () => {
  it("Move to bottom from the left strip: the selected tab of the last group there, a hidden region shown, the same view", async () => {
    const host = await show();
    arrange([[FILES, SEARCH, PROBE]], [], [[OTHER]]);
    layout.setBottomHidden(true);
    selectView(PROBE);
    flushSync();
    const field = host.querySelector<HTMLInputElement>(".probe-field");
    if (!field) throw new Error("no probe body");
    field.value = "typed";
    press(stripButtons("sidebar")[0]);
    await pick("Move to bottom");
    expect(placed()).toEqual({ left: [[FILES, SEARCH]], right: [], bottom: [[OTHER, PROBE]] });
    expect(arrangement.isSelected(PROBE)).toBe(true);
    expect(layout.bottomHidden).toBe(false);
    // never created again
    expect(hostOf(PROBE)).toBe(host);
    expect(regionAt(host)).toBe("bottom");
    expect(probe.mounts).toBe(1);
    expect(field.value).toBe("typed");
    expect(said()).toBe("Outline moved to the bottom");
    // the keyboard: the options of the moved view, at the end of the strip it joined
    const [button] = stripButtons("bottom");
    expect(button?.getAttribute("data-view-menu")).toBe(PROBE);
    expect(document.activeElement).toBe(button);
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });

  it("Move to left | right: the selected tab of the last group there", async () => {
    await show();
    press(headerButton(PROBE));
    await pick("Move to left");
    expect(placed()).toEqual({ left: [[FILES, SEARCH, PROBE]], right: [[OTHER]], bottom: [] });
    expect(arrangement.isSelected(PROBE)).toBe(true);
    expect(said()).toBe("Outline moved to the left");
    expect(document.activeElement).toBe(stripButtons("sidebar")[0]);
    expect(document.activeElement?.getAttribute("data-view-menu")).toBe(PROBE);

    layout.setOpen(OTHER, false);
    flushSync();
    press(stripButtons("sidebar")[0]);
    await pick("Move to right");
    expect(placed()).toEqual({ left: [[FILES, SEARCH]], right: [[OTHER, PROBE]], bottom: [] });
    expect(said()).toBe("Outline moved to the right");
    // alone among the present views of its group: its own header has the options
    expect(document.activeElement).toBe(headerButton(PROBE));
  });

  it("at the bottom: Show on the right, then Move left; each said, the keyboard on the view's options", async () => {
    await show();
    arrange([[FILES]], [], [[SEARCH, PROBE, OTHER]]);
    selectView(PROBE);
    flushSync();
    press(stripButtons("bottom")[0]);
    await pick("Show on the right");
    expect(placed().bottom).toEqual([[SEARCH, OTHER], [PROBE]]);
    expect(said()).toBe("Outline now shows on the right");
    expect(document.activeElement).toBe(headerButton(PROBE));
    press(headerButton(PROBE));
    await pick("Move left");
    expect(placed().bottom).toEqual([[PROBE], [SEARCH, OTHER]]);
    expect(said()).toBe("Outline moved left");
    expect(document.activeElement).toBe(headerButton(PROBE));
    press(headerButton(PROBE));
    await pick("Move right");
    expect(said()).toBe("Outline moved right");
    press(stripButtons("bottom")[0]);
    await pick("Show on the left");
    expect(placed().bottom).toEqual([[SEARCH], [OTHER], [PROBE]]);
    expect(said()).toBe("Search now shows on the left");
  });

  it("at a side: Show as tab selects the tab, the strip's options take the keyboard", async () => {
    await show();
    press(headerButton(PROBE));
    await pick("Show as tab");
    expect(placed().right).toEqual([[OTHER, PROBE]]);
    expect(arrangement.isSelected(PROBE)).toBe(true);
    expect(said()).toBe("Outline is now a tab");
    expect(document.activeElement).toBe(stripButtons("right")[0]);
    press(stripButtons("right")[0]);
    await pick("Show above");
    expect(placed().right).toEqual([[PROBE], [OTHER]]);
    expect(said()).toBe("Outline moved above");
    expect(document.activeElement).toBe(headerButton(PROBE));
  });

  it("Reset layout: the three regions back to the default, said; the keyboard stays with the view", async () => {
    await show();
    arrange([[SEARCH], [FILES]], [[OTHER]], [[PROBE]]);
    const reset = vi.spyOn(arrangement, "reset");
    press(stripButtons("sidebar")[0]);
    await pick("Reset layout");
    expect(reset).toHaveBeenCalledOnce();
    expect(arrangement.saved).toEqual(defaultArrangement());
    expect(said()).toBe("Layout of the panels reset");
    // Search is a tab behind Files again: its tab takes the keyboard (never the page body)
    expect(document.activeElement).toBe(
      document.querySelector(`[role="tab"][data-view="${SEARCH}"]`),
    );
    // nothing more to reset
    press(stripButtons("sidebar")[0]);
    const again = menuItems().find((item) => item.textContent?.trim() === "Reset layout");
    expect(again?.getAttribute("aria-disabled")).toBe("true");
  });

  it("Close from a strip's options: the view goes, the keyboard goes to what its region still shows", async () => {
    await show();
    arrange([[FILES, SEARCH]], [[PROBE, OTHER]]);
    selectView(OTHER);
    flushSync();
    press(stripButtons("right")[0]);
    await pick("Close");
    expect(arrangement.isPresent(OTHER)).toBe(false);
    expect(document.activeElement).not.toBe(document.body);
    expect(slot(PROBE)?.contains(document.activeElement)).toBe(true);
  });
});
