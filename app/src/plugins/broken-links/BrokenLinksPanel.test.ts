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
  mock.probe.project.files.delete("@wt/feature/docs/bad.md");
  mock.probe.project.files.delete("@wt/a-very-long-worktree-name-for-the-layout/x.md");
  mock.probe.project.files.delete("docs/many.md");
  mock.probe.project.worktrees.length = 0;
  brokenLinks.report = null;
  vi.restoreAllMocks();
});

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
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
    // one root: no section headings
    expect(target.querySelector(".root")).toBeNull();
  });

  it("counts the opened folder alone and shows each linked worktree apart (#62)", async () => {
    mock.probe.project.add("docs/bad.md", "# Bad\n\n[gone](missing.md)\n");
    mock.probe.project.add("@wt/feature/docs/bad.md", "[a](gone-a.md)\n\n[b](gone-b.md)\n");
    stop = brokenLinks.start();
    component = mount(Panel, { target });
    flushSync();
    await until(() => target.querySelector(".root") !== null, "the sections");
    expect(target.querySelector("header .count")?.textContent).toBe("1");
    const text = (selector: string): string[] =>
      Array.from(target.querySelectorAll(selector)).map(
        (e) => e.textContent?.replace(/\s+/g, " ").trim() ?? "",
      );
    expect(text(".root .root-name")).toEqual(["This folder", "Linked worktree feature"]);
    expect(text(".root .n")).toEqual(["1", "2"]);
    // the number is said as a count of broken links
    expect(text(".root .sr-only")).toEqual(["1 broken link", "2 broken links"]);
    // the opened folder first; a worktree's folder is shown without its prefix, the full path stays in the tooltip
    expect(text(".doc .folder")).toEqual(["docs", "docs"]);
    const heads = Array.from(target.querySelectorAll<HTMLElement>(".doc-head"));
    expect(heads.map((h) => h.title)).toEqual(["docs/bad.md", "@wt/feature/docs/bad.md"]);
    const navigate = vi.spyOn(documents, "navigate").mockImplementation(() => {});
    heads[1]?.querySelector<HTMLElement>(".doc")?.click();
    expect(navigate).toHaveBeenCalledWith("@wt/feature/docs/bad.md", null);
  });

  it("says the opened folder has none when only a worktree has broken links", async () => {
    mock.probe.project.add("@wt/feature/docs/bad.md", "[a](gone-a.md)\n");
    stop = brokenLinks.start();
    component = mount(Panel, { target });
    flushSync();
    await until(() => target.querySelector(".root") !== null, "the sections");
    expect(target.querySelector("header .count")).toBeNull();
    expect(target.textContent).toContain("No broken links in this folder");
    expect(target.textContent).toContain("Linked worktree feature");
  });

  it("gives a long worktree name its whole text in a tooltip", async () => {
    const name = "a-very-long-worktree-name-for-the-layout";
    mock.probe.project.add(`@wt/${name}/x.md`, "[a](gone.md)\n");
    stop = brokenLinks.start();
    component = mount(Panel, { target });
    flushSync();
    await until(() => target.querySelector(".root") !== null, "the sections");
    const names = Array.from(target.querySelectorAll<HTMLElement>(".root .root-name"));
    expect(names.map((n) => n.title)).toEqual(["This folder", `Linked worktree ${name}`]);
  });

  it("says when a worktree's links fall past the list limit, and what the note counts", async () => {
    mock.probe.project.worktrees.push("feature");
    const many = Array.from({ length: 500 }, (_, i) => `[l${i}](gone-${i}.md)`).join("\n");
    mock.probe.project.add("docs/many.md", `${many}\n`);
    mock.probe.project.add("@wt/feature/docs/bad.md", "[a](gone-a.md)\n");
    stop = brokenLinks.start();
    component = mount(Panel, { target });
    flushSync();
    await until(() => target.querySelector(".root") !== null, "the sections");
    expect(target.querySelector("header .count")?.textContent).toBe("500");
    const sections = Array.from(target.querySelectorAll(".root"));
    expect(sections).toHaveLength(2);
    expect(target.textContent).toContain("Not listed: the list shows 500 links at most.");
    expect(target.textContent).toContain(
      "Showing the first 500 of 501 broken links in this folder and its linked worktrees.",
    );
  });

  it("keeps the short note when only the opened folder has broken links", async () => {
    const many = Array.from({ length: 501 }, (_, i) => `[l${i}](gone-${i}.md)`).join("\n");
    mock.probe.project.add("docs/many.md", `${many}\n`);
    stop = brokenLinks.start();
    component = mount(Panel, { target });
    flushSync();
    await until(() => target.querySelector(".link") !== null, "the list");
    expect(target.textContent).toContain("Showing the first 500 of 501 broken links.");
    expect(target.textContent).not.toContain("Not listed");
  });

  it("shows no count before the first scan is done", () => {
    component = mount(Panel, { target });
    brokenLinks.report = {
      ready: false,
      docs: [],
      total: 12_345,
      current: 12_345,
      roots: [{ prefix: "", total: 12_345 }],
      truncated: false,
      skipped: 0,
    };
    flushSync();
    expect(target.querySelector(".count")).toBeNull();
    expect(target.querySelector("[role=status]")).not.toBeNull();
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
