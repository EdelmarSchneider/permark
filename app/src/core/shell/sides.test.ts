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

const { reading } = await import("../doc/reading.svelte");
const { layout } = await import("./layout.svelte");
const { ui } = await import("./ui.svelte");
const { toggleOutlinePanel, toggleRightPanel } = await import("./sides");
const { resolveKey, isTypingTarget } = await import("../commands/dispatch");
const { buildKeymap } = await import("../commands/keymap");

afterEach(() => {
  ui.right = null;
  layout.setRightHidden(false);
  layout.setSidebarHidden(false);
  reading.setOutline(true);
  ui.resetFocus();
});

describe("outline toggle while the right panel is hidden and a plugin panel is chosen", () => {
  it("shows the outline (spec shell.md: 'shows again with that content')", () => {
    ui.right = "live-feed.panel";
    reading.setOutline(false);
    layout.setRightHidden(true);
    toggleOutlinePanel();
    expect(layout.rightHidden).toBe(false);
    expect(reading.outline).toBe(true);
    // App.svelte draws the plugin panel whenever ui.right !== null: the outline only shows if the panel let go
    expect(ui.right, "the plugin panel would show instead of the outline").toBeNull();
  });

  it("a plugin panel toggle while hidden shows that panel (not the outline)", () => {
    layout.setRightHidden(true);
    ui.toggleRight("live-feed.panel");
    expect(layout.rightHidden).toBe(false);
    expect(ui.right).toBe("live-feed.panel");
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
