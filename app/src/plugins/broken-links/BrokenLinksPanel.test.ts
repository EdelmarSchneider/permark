// @vitest-environment jsdom
// The Broken links tab, mounted against the mock backend; the tabs of the sidebar when many plugins add one.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER } from "../../api/mock";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});

const { api } = await import("../../api");
const { default: Panel } = await import("./BrokenLinksPanel.svelte");
const { brokenLinks } = await import("./store.svelte");
const { windowStore } = await import("../../core/window.svelte");
const { documents } = await import("../../core/doc/document.svelte");
const { default: SidebarTabs } = await import("../../core/search/SidebarTabs.svelte");
const { contributions } = await import("../../core/plugins/instance.svelte");
const { ui } = await import("../../core/shell/ui.svelte");
const { NO_REQUIREMENT } = await import("../../core/conditions/available");
const { conditions } = await import("../../core/conditions/context.svelte");
const { syncPlugins } = await import("../../core/plugins/instance.svelte");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;

let target: HTMLElement;
let component: Record<string, unknown> | null = null;
let stop: (() => void) | null = null;
let started = false;

beforeEach(async () => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
  await mock.openPath(MOCK_FOLDER, "thisTab");
  if (!started) {
    await windowStore.start();
    started = true;
  }
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  stop?.();
  stop = null;
  mock.probe.project.files.delete("docs/bad.md");
  vi.restoreAllMocks();
});

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 3000, interval: 10 });
}

describe("the Broken links tab", () => {
  it("says nothing is broken, then lists the links by document with their lines", async () => {
    stop = brokenLinks.start();
    component = mount(Panel, { target });
    flushSync();
    await until(() => target.textContent?.includes("No broken links") === true, "none");
    mock.probe.project.add("docs/bad.md", "# Bad\n\n[gone](missing.md)\n");
    await brokenLinks.load();
    flushSync();
    expect(target.querySelector(".count")?.textContent).toBe("1");
    expect(target.querySelector("h3 .name")?.textContent).toBe("bad.md");
    expect(target.querySelector(".written")?.textContent).toBe("missing.md");
    expect(target.querySelector(".line")?.textContent).toBe("3");
  });

  it("opens the document, and at the line of a link", async () => {
    mock.probe.project.add("docs/bad.md", "# Bad\n\n[gone](missing.md)\n");
    stop = brokenLinks.start();
    component = mount(Panel, { target });
    flushSync();
    await until(() => target.querySelector(".link") !== null, "the list");
    const navigate = vi.spyOn(documents, "navigate").mockImplementation(() => {});
    target.querySelector<HTMLElement>(".link")?.click();
    expect(navigate).toHaveBeenCalledWith("docs/bad.md", null, 3);
    target.querySelector<HTMLElement>(".doc")?.click();
    expect(navigate).toHaveBeenLastCalledWith("docs/bad.md", null);
  });
});

describe("the tabs of the sidebar", () => {
  it("keep their words while there are few, and show the icon alone when crowded", async () => {
    const tab = (owner: string, id: string): (() => void) =>
      contributions.register({
        kind: "panel",
        id,
        owner,
        side: "left",
        label: "unread.title",
        requires: NO_REQUIREMENT,
        view: () => import("./BrokenLinksPanel.svelte"),
      });
    // the window started plugins of its own in the background: with them still arriving, the strip may be crowded
    syncPlugins([]);
    conditions.set({ ...conditions.current, plugins: [] });
    const removals = [tab("a", "a.panel"), tab("b", "b.panel")];
    component = mount(SidebarTabs, { target });
    flushSync();
    const labels = (): string[] =>
      Array.from(target.querySelectorAll('[role="tab"]')).map((t) => t.textContent?.trim() ?? "");
    // Files, Search and two plugin tabs: crowded, so only the open tab keeps its words
    expect(labels()[0]).toContain("Files");
    expect(labels()[1]).toBe("");
    expect(target.querySelector<HTMLElement>('[role="tab"]:nth-child(2)')?.title).toBe("Search");
    ui.showSidebar("search");
    flushSync();
    expect(labels()[1]).toContain("Search");
    for (const remove of removals) remove();
    flushSync();
    expect(labels().every((l) => l !== "")).toBe(true);
    ui.showSidebar("files");
  });
});
