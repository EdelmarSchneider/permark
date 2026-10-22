// @vitest-environment jsdom
// Outline marks (#9): a dot after the label of a changed section, words for screen readers, the first plugin that
// says something wins, gone when no plugin marks it.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { OutlineItem } from "../../api";
import type { PluginId } from "../../api/generated/PluginId";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});

const { NO_REQUIREMENT } = await import("../conditions/available");
const { contributions } = await import("../plugins/instance.svelte");
const OutlinePanel = (await import("./OutlinePanel.svelte")).default;

const removals: (() => void)[] = [];
let component: Record<string, unknown> | null = null;

const items: OutlineItem[] = [
  { label: "Intro", level: 1, id: "intro", line: 1, block: "b1" },
  { label: "Plan", level: 2, id: "plan", line: 5, block: "b2" },
];

function marks(owner: string, at: readonly number[], text = "changed"): void {
  removals.push(
    contributions.register({
      kind: "outlineMark",
      id: `${owner}.changed`,
      owner: owner as PluginId,
      requires: NO_REQUIREMENT,
      mark: (index) => (at.includes(index) ? { text, title: "Changed in the last update" } : null),
    }),
  );
}

function show(): HTMLElement {
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(OutlinePanel, { target, props: { items } });
  flushSync();
  return target;
}

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
  for (const remove of removals.splice(0)) remove();
});

describe("outline marks", () => {
  it("draw nothing without a plugin that marks", () => {
    expect(show().querySelector("[data-mark]")).toBeNull();
  });

  it("put a dot with words after the changed section only, and say it in the tooltip", () => {
    marks("live-feed", [1]);
    const root = show();
    const rows = [...root.querySelectorAll<HTMLButtonElement>("button.item")];
    expect(rows[0]?.querySelector("[data-mark]")).toBeNull();
    expect(rows[1]?.querySelector("[data-mark]")?.textContent).toBe("changed");
    expect(rows[1]?.title).toBe("Plan · Changed in the last update");
    expect(rows[0]?.title).toBe("Intro");
  });

  it("draw one dot when two plugins mark the same section", () => {
    marks("live-feed", [0], "first");
    marks("review", [0], "second");
    const root = show();
    const dots = root.querySelectorAll("button.item [data-mark]");
    expect(dots).toHaveLength(1);
    expect(dots[0]?.textContent).toBe("first");
  });
});
