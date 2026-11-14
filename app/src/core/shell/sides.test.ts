// @vitest-environment jsdom
// Adversarial: the side-panel toggles against the stores (outline | plugin panel while the right panel is hidden,
// Ctrl+B in each editor, storms of toggles).
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
const { hasSidebar, showView, toggleOutlinePanel, togglePanel } = await import("./sides");
const { arrangement } = await import("./arrangement.svelte");
const { FILES, OUTLINE, SEARCH } = await import("./arrangement");
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
});

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
