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
const { toggleOutlinePanel, togglePanel } = await import("./sides");
const { resolveKey, isTypingTarget } = await import("../commands/dispatch");
const { buildKeymap } = await import("../commands/keymap");

afterEach(() => {
  // a single file: nothing read, everything back to its start (views, hidden panels)
  void layout.load(api, false);
  reading.setOutline(true);
  ui.resetFocus();
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
