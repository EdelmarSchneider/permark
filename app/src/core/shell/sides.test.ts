// @vitest-environment jsdom
// Adversarial: the region toggles against the stores (outline | plugin panel while its region is hidden, wherever
// the view is arranged; the bottom region's toggle; Ctrl+B in each editor, storms of toggles).
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});

const { api } = await import("../../api");
const { reading } = await import("../doc/reading.svelte");
const { layout } = await import("./layout.svelte");
const { ui } = await import("./ui.svelte");
const {
  hasSidebar,
  hasToggle,
  moveToRegion,
  regionState,
  showView,
  toggleBottomPanel,
  toggleOutlinePanel,
  togglePanel,
  toggleRightPanel,
  toggleSidebar,
} = await import("./sides");
const { BOTTOM, PAGE_MIN_H, regionShown } = await import("./layout");
const { arrangement } = await import("./arrangement.svelte");
const { defaultArrangement, FILES, LIVE_VIEW, moveTo, OUTLINE, SEARCH } =
  await import("./arrangement");
const { conditions, neutralContext } = await import("../conditions/context.svelte");
const { contributions } = await import("../plugins/instance.svelte");
const { NO_REQUIREMENT } = await import("../conditions/available");
const { resolveKey, isTypingTarget } = await import("../commands/dispatch");
const { buildKeymap } = await import("../commands/keymap");

afterEach(() => {
  // a single file: nothing read, everything back to its start (views, hidden panels)
  void layout.load(api, false);
  reading.setOutline(true);
  ui.resetFocus();
  conditions.set(neutralContext());
  arrangement.arrange(defaultArrangement());
  layout.setCenterHeight(0);
  for (const remove of removals.splice(0)) remove();
});

const removals: (() => void)[] = [];
/** The live feed's panel is offered (a view the person opens | closes; by default arranged at the right). */
function offerLive(): void {
  removals.push(
    contributions.register({
      kind: "panel",
      id: LIVE_VIEW,
      owner: "live-feed",
      side: "right",
      label: "live.label",
      requires: NO_REQUIREMENT,
      view: () => import("../testing/StripFirst.svelte"),
    }),
  );
}
/** Which regions the person hid: left, right, bottom. */
const hidden = (): boolean[] => [layout.sidebarHidden, layout.rightHidden, layout.bottomHidden];
function hideAll(): void {
  layout.setSidebarHidden(true);
  layout.setRightHidden(true);
  layout.setBottomHidden(true);
}

describe("showing a view", () => {
  it("of the sidebar: its tab is selected, a hidden sidebar shows again, no open flag is written", () => {
    conditions.set({ ...neutralContext(), workspace: "project" });
    layout.setSidebarHidden(true);
    const asked = ui.searchFocus;
    showView(SEARCH);
    expect(layout.sidebarHidden).toBe(false);
    expect(ui.frameMoving).toBe(true);
    expect([FILES, SEARCH].map((id) => arrangement.isSelected(id))).toEqual([false, true]);
    expect(arrangement.onScreen(SEARCH)).toBe(true);
    // the tab is the project's choice, kept with the tabs of the other regions
    expect(layout.views.selected).toEqual([SEARCH]);
    // Files and Search are always open in a project: nothing to remember
    expect(layout.views.open).toEqual({});
    // the Search box is asked to take the cursor
    expect(ui.searchFocus).toBe(asked + 1);
    showView(FILES);
    expect([FILES, SEARCH].map((id) => arrangement.isSelected(id))).toEqual([true, false]);
    expect(layout.views.open).toEqual({});
    expect(ui.searchFocus).toBe(asked + 1);
  });

  it("that can be closed: opened, selected, its own region shown", () => {
    const remove = contributions.register({
      kind: "panel",
      id: "live-feed.panel",
      owner: "live-feed",
      side: "right",
      label: "live.label",
      requires: NO_REQUIREMENT,
      view: () => import("../testing/StripFirst.svelte"),
    });
    layout.setRightHidden(true);
    showView("live-feed.panel");
    expect(layout.rightHidden).toBe(false);
    expect(layout.views.open).toEqual({ "live-feed.panel": true });
    expect(layout.views.selected[0]).toBe("live-feed.panel");
    reading.setOutline(false);
    showView(OUTLINE);
    expect(reading.outline).toBe(true);
    expect(layout.views.selected[0]).toBe(OUTLINE);
    remove();
  });

  it("nobody offers now: nothing changes", () => {
    layout.setSidebarHidden(true);
    showView("gone.panel");
    expect(layout.sidebarHidden).toBe(true);
    expect(layout.views).toEqual({ v: 1, open: {}, selected: [] });
  });
});

describe("the left region of a window", () => {
  const showing = (kind: "project" | "singleFile" | "notes") =>
    ({ state: "showing", workspace: { kind }, document: null }) as never;

  it("is there with a view arranged at the left that is present, and always in the Notes", () => {
    // a single file: nothing at the left
    expect(hasSidebar(showing("singleFile"))).toBe(false);
    expect(hasSidebar(showing("notes"))).toBe(true);
    conditions.set({ ...neutralContext(), workspace: "project" });
    expect(hasSidebar(showing("project"))).toBe(true);
    expect(hasSidebar({ state: "welcome" })).toBe(false);
  });
});

describe("the outline | a panel toggle while the right panel is hidden", () => {
  it("the outline toggle shows the region again with the outline; an open panel stays open", () => {
    layout.setOpen("live-feed.panel", true);
    reading.setOutline(false);
    layout.setRightHidden(true);
    toggleOutlinePanel();
    expect(layout.rightHidden).toBe(false);
    expect(reading.outline).toBe(true);
    expect(layout.views.open["live-feed.panel"]).toBe(true);
    expect(layout.views.selected[0]).toBe("outline");
  });

  it("a panel toggle while hidden shows the region with that panel open (the outline choice stays)", () => {
    reading.setOutline(false);
    layout.setRightHidden(true);
    togglePanel("live-feed.panel");
    expect(layout.rightHidden).toBe(false);
    expect(layout.views.open["live-feed.panel"]).toBe(true);
    expect(reading.outline).toBe(false);
  });
});

describe("a view arranged in another region", () => {
  it("the outline at the left | the bottom: its toggle shows THAT region, the others stay hidden", () => {
    for (const [region, after] of [
      ["left", [false, true, true]],
      ["bottom", [true, true, false]],
    ] as const) {
      arrangement.arrange(moveTo(defaultArrangement(), OUTLINE, region));
      hideAll();
      reading.setOutline(false);
      toggleOutlinePanel();
      expect(reading.outline, region).toBe(true);
      expect(hidden(), region).toEqual(after);
      expect(layout.views.selected[0], region).toBe(OUTLINE);
    }
  });

  it("the outline on in a hidden region: its toggle shows the region, never turns the outline off", () => {
    arrangement.arrange(moveTo(defaultArrangement(), OUTLINE, "bottom"));
    hideAll();
    toggleOutlinePanel();
    expect(reading.outline).toBe(true);
    expect(hidden()).toEqual([true, true, false]);
  });

  it("a panel at the bottom: its toggle opens it and shows the bottom region; on screen there, it closes", () => {
    offerLive();
    arrangement.arrange(moveTo(defaultArrangement(), LIVE_VIEW, "bottom"));
    hideAll();
    togglePanel(LIVE_VIEW);
    expect(layout.views.open[LIVE_VIEW]).toBe(true);
    expect(hidden()).toEqual([true, true, false]);
    // open behind a hidden region: brought forward, never closed
    layout.setBottomHidden(true);
    togglePanel(LIVE_VIEW);
    expect(layout.views.open[LIVE_VIEW]).toBe(true);
    expect(layout.bottomHidden).toBe(false);
    togglePanel(LIVE_VIEW);
    expect(layout.views.open[LIVE_VIEW]).toBe(false);
    // the right region hidden says nothing about a view of the bottom
    expect(layout.rightHidden).toBe(true);
  });

  it("shown from anywhere (its command): its own region shows", () => {
    offerLive();
    arrangement.arrange(moveTo(defaultArrangement(), LIVE_VIEW, "bottom"));
    hideAll();
    showView(LIVE_VIEW);
    expect(hidden()).toEqual([true, true, false]);
    expect(arrangement.onScreen(LIVE_VIEW)).toBe(true);
  });

  it("moved into a hidden region: the region shows, the view is the tab its group shows", () => {
    offerLive();
    layout.setOpen(LIVE_VIEW, true);
    hideAll();
    moveToRegion(LIVE_VIEW, "bottom");
    expect(arrangement.regionOf(LIVE_VIEW)).toBe("bottom");
    expect(arrangement.saved.bottom).toEqual([{ views: [LIVE_VIEW], share: 1 }]);
    expect(hidden()).toEqual([true, true, false]);
    expect(ui.frameMoving).toBe(true);
    // a second view joins its group as the tab shown
    conditions.set({ ...neutralContext(), workspace: "project" });
    moveToRegion(SEARCH, "bottom");
    expect(arrangement.saved.bottom).toEqual([{ views: [LIVE_VIEW, SEARCH], share: 1 }]);
    expect([LIVE_VIEW, SEARCH].map((id) => arrangement.isSelected(id))).toEqual([false, true]);
    // a view nobody offers now: nothing moves
    const before = arrangement.saved;
    moveToRegion("gone.panel", "left");
    expect(arrangement.saved).toBe(before);
  });
});

describe("the toggle of a region", () => {
  it("the bottom one: there only with a present view; it hides | shows, and turns nothing on", () => {
    offerLive();
    arrangement.arrange(moveTo(defaultArrangement(), LIVE_VIEW, "bottom"));
    // arranged there, closed: no region, no toggle; the key does nothing
    expect(hasToggle("bottom")).toBe(false);
    toggleBottomPanel();
    expect(layout.bottomHidden).toBe(false);
    expect(layout.views.open).toEqual({});
    layout.setOpen(LIVE_VIEW, true);
    expect(hasToggle("bottom")).toBe(true);
    expect(regionShown(regionState("bottom"))).toBe(true);
    toggleBottomPanel();
    expect(hidden()).toEqual([false, false, true]);
    expect(regionShown(regionState("bottom"))).toBe(false);
    expect(ui.frameMoving).toBe(true);
    toggleBottomPanel();
    expect(hidden()).toEqual([false, false, false]);
    for (let i = 0; i < 101; i++) toggleBottomPanel();
    expect(layout.bottomHidden).toBe(true);
    expect(layout.views.open).toEqual({ [LIVE_VIEW]: true });
  });

  it("the bottom one in a window too short for the region: off as if hidden, its views not on screen; its toggle changes nothing", () => {
    offerLive();
    layout.setOpen(LIVE_VIEW, true);
    arrangement.arrange(moveTo(defaultArrangement(), LIVE_VIEW, "bottom"));
    expect(regionShown(regionState("bottom"))).toBe(true);
    expect(arrangement.onScreen(LIVE_VIEW)).toBe(true);
    layout.setCenterHeight(PAGE_MIN_H + BOTTOM.min - 1);
    expect(hasToggle("bottom")).toBe(true);
    expect(regionShown(regionState("bottom"))).toBe(false);
    expect(arrangement.onScreen(LIVE_VIEW)).toBe(false);
    // the side regions are as they were
    expect(regionState("right").hidden).toBe(false);
    // nothing to show in the room there is: the person's choice stays
    toggleBottomPanel();
    expect(layout.bottomHidden).toBe(false);
    expect(regionShown(regionState("bottom"))).toBe(false);
    layout.setCenterHeight(PAGE_MIN_H + BOTTOM.min);
    expect(regionShown(regionState("bottom"))).toBe(true);
    expect(arrangement.onScreen(LIVE_VIEW)).toBe(true);
  });

  it("the right one counts the views arranged at the right: with nothing present it opens the first it could show", () => {
    offerLive();
    reading.setOutline(false);
    // the outline arranged elsewhere: the right region has the panel alone to offer
    arrangement.arrange(moveTo(defaultArrangement(), OUTLINE, "left"));
    expect(hasToggle("right")).toBe(true);
    toggleRightPanel();
    expect(layout.views.open[LIVE_VIEW]).toBe(true);
    expect(reading.outline).toBe(false);
    expect(regionShown(regionState("right"))).toBe(true);
    toggleRightPanel();
    expect(layout.rightHidden).toBe(true);
    // the panel moved away too: nothing at the right it could show
    arrangement.arrange(moveTo(arrangement.saved, LIVE_VIEW, "bottom"));
    layout.setRightHidden(false);
    expect(hasToggle("right")).toBe(false);
    toggleRightPanel();
    expect(layout.rightHidden).toBe(false);
  });

  it("the right one with a view that is always open (Search moved there): hides | shows, turns nothing on", () => {
    conditions.set({ ...neutralContext(), workspace: "project" });
    reading.setOutline(false);
    arrangement.arrange(moveTo(defaultArrangement(), SEARCH, "right"));
    expect(hasToggle("right")).toBe(true);
    toggleRightPanel();
    expect(layout.rightHidden).toBe(true);
    toggleRightPanel();
    expect(layout.rightHidden).toBe(false);
    expect(reading.outline).toBe(false);
    expect(layout.views.open).toEqual({});
  });

  it("the left one: where the window has a left region; never in a window without one", () => {
    // no workspace on screen: nothing at the left
    expect(hasToggle("left")).toBe(false);
    toggleSidebar();
    expect(layout.sidebarHidden).toBe(false);
  });
});

describe("Ctrl+B by editor", () => {
  const map = buildKeymap([{ command: "sidebar.toggle", keys: "Mod+b" }], {}, ["sidebar.toggle"]);
  const command = {
    id: "sidebar.toggle",
    label: "x",
    run: () => {},
    owner: "core",
    requires: { needs: [], prefers: [] },
  } as never;
  const env = (target: Element) => ({
    mac: false,
    keymap: map,
    commands: [command],
    context: {} as never,
    typing: isTypingTarget(target),
    overlay: "none" as const,
    dialog: false,
  });
  const ev = (defaultPrevented: boolean) => ({
    key: "b",
    code: "KeyB",
    ctrlKey: true,
    metaKey: false,
    altKey: false,
    shiftKey: false,
    defaultPrevented,
  });

  it("visual editor (ProseMirror takes it: bold) -> passes", () => {
    const pm = document.createElement("div");
    pm.setAttribute("contenteditable", "true");
    expect(resolveKey(ev(true), env(pm)).kind).toBe("pass");
  });

  it("source editor (CodeMirror takes Mod-b: bold | nothing outside Markdown) -> passes", () => {
    const cm = document.createElement("div");
    cm.className = "cm-content";
    expect(resolveKey(ev(true), env(cm)).kind).toBe("pass");
  });

  it("outside the editors (page, panels) -> the sidebar toggles", () => {
    const page = document.createElement("div");
    expect(resolveKey(ev(false), env(page)).kind).toBe("run");
  });
});
