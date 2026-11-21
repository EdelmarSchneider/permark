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
const { documents } = await import("./document.svelte");
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

function show(shown: readonly OutlineItem[] = items): HTMLElement {
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(OutlinePanel, { target, props: { items: shown } });
  flushSync();
  return target;
}

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  documents.currentHeading = -1;
  document.body.replaceChildren();
  for (const remove of removals.splice(0)) remove();
  vi.restoreAllMocks();
  Reflect.deleteProperty(Element.prototype, "scrollIntoView");
});

describe("the current heading", () => {
  it("in a long outline: a change marks the new row and unmarks the old one, the other rows never look", () => {
    const scrolled: Element[] = [];
    // jsdom draws nothing and has no scrollIntoView
    Object.defineProperty(Element.prototype, "scrollIntoView", {
      configurable: true,
      value: function (this: Element) {
        scrolled.push(this);
      },
    });
    const many: OutlineItem[] = Array.from({ length: 5000 }, (_, i) => ({
      label: `Section ${i}`,
      level: 1 + (i % 3),
      id: `s${i}`,
      line: 1 + i * 4,
      block: `b${i}`,
    }));
    documents.currentHeading = 10;
    const root = show(many);
    const rows = [...root.querySelectorAll<HTMLButtonElement>("button.item")];
    expect(rows).toHaveLength(5000);
    expect(rows[10]?.classList.contains("is-current")).toBe(true);
    expect(rows[10]?.getAttribute("aria-current")).toBe("location");
    expect(root.querySelectorAll(".is-current, [aria-current]")).toHaveLength(1);
    expect(scrolled).toEqual([rows[10]]);
    const reads = vi.spyOn(documents, "currentHeading", "get");
    const touched = new Set<Node>();
    const watch = new MutationObserver((records) => {
      for (const record of records) touched.add(record.target);
    });
    watch.observe(root, { attributes: true, childList: true, subtree: true });
    documents.currentHeading = 4000;
    flushSync();
    for (const record of watch.takeRecords()) touched.add(record.target);
    watch.disconnect();
    expect([...touched]).toEqual(expect.arrayContaining([rows[10], rows[4000]]));
    expect(touched.size).toBe(2);
    expect(rows[4000]?.classList.contains("is-current")).toBe(true);
    expect(rows[4000]?.getAttribute("aria-current")).toBe("location");
    expect(rows[10]?.classList.contains("is-current")).toBe(false);
    expect(rows[10]?.hasAttribute("aria-current")).toBe(false);
    expect(scrolled).toEqual([rows[10], rows[4000]]);
    // one reader of the current heading, not one per row
    expect(reads.mock.calls.length).toBeLessThanOrEqual(2);
    // before the first heading: no row is current
    documents.currentHeading = -1;
    flushSync();
    expect(root.querySelectorAll(".is-current, [aria-current]")).toHaveLength(0);
  });

  it("the current mark follows data-at: the row found by place is the heading's", () => {
    Object.defineProperty(Element.prototype, "scrollIntoView", { configurable: true, value() {} });
    const ten: OutlineItem[] = Array.from({ length: 10 }, (_, i) => ({
      label: `Section ${i}`,
      level: 1 + (i % 2),
      id: `s${i}`,
      line: 1 + i * 4,
      block: `b${i}`,
    }));
    // the row's own index, beside each mark
    const current = (root: HTMLElement): (string | null)[][] =>
      [...root.querySelectorAll<HTMLElement>(".is-current, [aria-current]")].map((row) => [
        row.dataset["at"] ?? null,
        row.getAttribute("aria-current"),
        row.classList.contains("is-current") ? "is-current" : null,
      ]);
    documents.currentHeading = 3;
    const root = show(ten);
    expect(current(root)).toEqual([["3", "location", "is-current"]]);
    documents.currentHeading = 7;
    flushSync();
    expect(current(root)).toEqual([["7", "location", "is-current"]]);
  });
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
