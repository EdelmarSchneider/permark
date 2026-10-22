// @vitest-environment jsdom
// Git per included project on screen (`projects.md` §12.2-12.4, §12.7, round 3): the node's branch, letters inside a
// project, the card's git block, Settings rows, the status item, git missing | can't read | still checking, the move
// question of a project renamed while PerMark was closed. Mounted against the mock backend with the git plugin on.
import { flushSync, mount, tick, unmount, type Component } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RepoSummary } from "../../api";
import { MOCK_FOLDER } from "../../api/mock";
import { MOCK_PROJECTS_LISTED, MOCK_PROJECTS_MANY } from "../../api/mockProjects";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});
Element.prototype.scrollIntoView = () => {};
Element.prototype.getBoundingClientRect = function () {
  return {
    left: 0,
    top: 0,
    right: 300,
    bottom: 400,
    width: 300,
    height: 400,
    x: 0,
    y: 0,
  } as DOMRect;
};
if (typeof CSS === "undefined" || typeof CSS.escape !== "function") {
  (globalThis as { CSS?: unknown }).CSS = { escape: (s: string) => s.replace(/["\\]/g, "\\$&") };
}

const { api } = await import("../../api");
const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const { default: ExplorerPanel } = await import("../../core/explorer/ExplorerPanel.svelte");
const { ExplorerStore } = await import("../../core/explorer/store.svelte");
const { default: ProjectsSection } = await import("../../core/projects/ProjectsSection.svelte");
const { default: ProjectStrip } = await import("../../core/projects/ProjectStrip.svelte");
const { default: GitStatus } = await import("./GitStatus.svelte");
const { contributions } = await import("../../core/plugins/instance.svelte");
const { conditions, neutralContext } = await import("../../core/conditions/context.svelte");
const { documents } = await import("../../core/doc/document.svelte");
const { ui } = await import("../../core/shell/ui.svelte");
const { projects } = await import("../../core/projects/instance.svelte");
const { windowStore } = await import("../../core/window.svelte");
const { members } = await import("./members.svelte");

const HUB = "k7m2q7xd2a";
const NOTES = "notes2note";

let target: HTMLElement;
let cleanups: (() => void)[] = [];
let started = false;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
}

/** Opens a folder in the mock with the git plugin on, following its projects. */
async function openFolder(root: string): Promise<void> {
  await mock.openPath(root, "thisTab");
  if (!started) {
    await windowStore.start();
    started = true;
  }
  conditions.set({ ...neutralContext(), workspace: "project", plugins: ["git"] });
  cleanups.push(await projects.start(root));
  // the window's plugin host turns the git plugin on (on by default)
  await until(() => contributions.ownedBy("git").some((c) => c.kind === "projectFacts"), "git on");
  await until(() => members.repos.size > 0 || root === MOCK_FOLDER, "repositories known");
}

async function explorer(): Promise<{ store: InstanceType<typeof ExplorerStore> }> {
  const store = new ExplorerStore(api, {
    fail: () => {},
    note: () => {},
    view: () => "main",
    linkUpdate: () => "never",
    openProject: () => {},
  });
  const stop = await store.start();
  const app = mount(ExplorerPanel, { target, props: { store, registry: contributions } });
  flushSync();
  await tick();
  cleanups.push(() => {
    void unmount(app);
    stop();
  });
  return { store };
}

async function expand(store: InstanceType<typeof ExplorerStore>, path: string): Promise<void> {
  if (!store.isOpen(path)) await store.toggle(path);
  flushSync();
}

function mountHere(component: Component<Record<string, never>>): void {
  const app = mount(component, { target });
  flushSync();
  cleanups.push(() => void unmount(app));
}

const node = (path: string): HTMLElement | null =>
  target.querySelector<HTMLElement>(`[data-path="${CSS.escape(path)}"]`);
const factOf = (path: string): string | null =>
  node(path)?.querySelector(".fact")?.textContent?.trim() ?? null;
const row = (path: string): HTMLElement | null =>
  target.querySelector<HTMLElement>(`[data-project="${CSS.escape(path)}"]`);
const interest = (level: "glance" | "detail"): string[] | null =>
  mock.probe.plugins.gitMembers.interest.filter((c) => c.level === level).at(-1)?.ids ?? null;

beforeEach(() => {
  mock.probe.projects.reset();
  mock.probe.plugins.gitMembers.interest.length = 0;
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  for (const clean of cleanups.reverse()) clean();
  cleanups = [];
  const probe = mock.probe.plugins.gitMembers;
  probe.unavailable(false);
  probe.hold(false);
  probe.shape(null);
  ui.closeSettingsPage();
  documents.setWorkspace(null);
});

describe("the project node", () => {
  it("shows the branch, muted, after the name; says it; plain folders and projects without git say nothing new", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    const { store } = await explorer();
    await expand(store, "hubs");
    await expand(store, "tools");
    await until(() => factOf("tools/notes-app") === "develop", "branch on the node");
    expect(factOf("hubs/hub-b")).toBe("main");
    expect(node("tools/notes-app")?.getAttribute("aria-label")).toBe(
      "notes-app, project, branch develop, 3 changed files inside",
    );
    // the fact is part of the name the reader hears: never read twice
    expect(node("tools/notes-app")?.querySelector(".fact")?.getAttribute("aria-hidden")).toBe(
      "true",
    );
    // not included | a plain folder: nothing
    expect(factOf("tools/new-app")).toBeNull();
    expect(factOf("tools")).toBeNull();
    // a project without git (.permark only): no branch text
    await expand(store, "hubs/hub-b");
    await expand(store, "hubs/hub-b/projects");
    await until(() => node("hubs/hub-b/projects/spec") !== null, "spec node");
    expect(factOf("hubs/hub-b/projects/spec")).toBeNull();
  });

  it("an open project node asks for its files: letters inside; closed, it lets go", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    const { store } = await explorer();
    await expand(store, "tools");
    await until(() => node("tools/notes-app") !== null, "node");
    expect(interest("glance") ?? []).not.toContain(NOTES);
    await expand(store, "tools/notes-app");
    await until(() => (interest("glance") ?? []).includes(NOTES), "glance interest");
    await until(
      () => node("tools/notes-app/README.md")?.textContent?.includes("M") === true,
      "letter inside the project",
    );
    await store.toggle("tools/notes-app");
    flushSync();
    await until(() => !(interest("glance") ?? [NOTES]).includes(NOTES), "interest dropped");
  });

  it("a plain folder: no fact, no interest with ids, status as before (A0)", async () => {
    await openFolder(MOCK_FOLDER);
    await explorer();
    mountHere(GitStatus);
    await new Promise((resolve) => setTimeout(resolve, 300));
    flushSync();
    expect(target.querySelector(".fact")).toBeNull();
    expect(mock.probe.plugins.gitMembers.interest.every((c) => c.ids.length === 0)).toBe(true);
    expect(target.querySelector(".prefix")).toBeNull();
  });
});

describe("the detail card", () => {
  it("branch, ahead | behind, changed files, last commit; details asked while it shows", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    const { store } = await explorer();
    await expand(store, "tools");
    await until(() => node("tools/notes-app") !== null, "node");
    projects.openCard({ path: "tools/notes-app", from: "menu", anchor: new DOMRect() });
    flushSync();
    await until(() => document.querySelector("[data-project-git]") !== null, "git block");
    await until(() => (interest("detail") ?? []).includes(NOTES), "detail interest");
    const block = (): string => document.querySelector("[data-project-git]")?.textContent ?? "";
    await until(() => block().includes("Last commit"), "details came");
    expect(block()).toContain("Branch develop");
    expect(block()).toContain("3 changed files");
    expect(block()).toContain("“Update the plan of notes-app”");
    const ahead = document.querySelector('[data-project-git] [role="img"]');
    expect(ahead?.textContent).toBe("↑1 ↓1");
    expect(ahead?.getAttribute("aria-label")).toBe(
      "1 commits ahead, 1 behind, as of the last fetch",
    );
    // the git dot's words are not repeated outside the block
    const card = document.querySelector('[role="dialog"]')!;
    expect(card.textContent).not.toContain("changed files inside");
    projects.closeCard();
    flushSync();
    await until(() => (interest("detail") ?? [NOTES]).length === 0, "detail dropped");
  });

  it("Checking git… while held; git missing and can't read: one sentence", async () => {
    await openFolder(MOCK_PROJECTS_MANY);
    const probe = mock.probe.plugins.gitMembers;
    probe.hold(true);
    const failed = probe.summaries().find((s) => s.error?.kind === "failed")!;
    const sample = probe.summaries().find((s) => s.error === null)!;
    await explorer();
    projects.openCard({ path: sample.path, from: "hover", anchor: new DOMRect() });
    flushSync();
    const block = (): string => document.querySelector("[data-project-git]")?.textContent ?? "";
    await until(() => block().includes("Checking git…"), "checking");
    probe.hold(false);
    probe.unavailable(true);
    await until(
      () => block().includes("Git isn't available, so branch and changes aren't shown."),
      "git missing",
    );
    probe.unavailable(false);
    projects.closeCard();
    flushSync();
    projects.openCard({ path: failed.path, from: "hover", anchor: new DOMRect() });
    flushSync();
    await until(() => block().includes("Git can't read this project."), "can't read");
    expect(block()).toContain("Git doesn't trust this folder's owner.");
  });
});

describe("Settings > Projects inside", () => {
  it("rows say branch · changes; the expanded row says whose repository; projects in view are asked", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    mountHere(ProjectsSection);
    await until(
      () => row("tools/notes-app")?.textContent?.includes("develop · 3 changed") === true,
      "row",
    );
    expect(row("hubs/hub-b")?.textContent).toContain("main · clean");
    // the git dot is not repeated next to its own words
    expect(row("tools/notes-app")?.textContent).not.toContain("•");
    await until(
      () => [HUB, NOTES].every((id) => (interest("glance") ?? []).includes(id)),
      "rows in view asked",
    );
    expect(target.textContent).toContain(
      "Included projects are followed from here: their changes, unread documents and branches show in this folder.",
    );
    const rules = Array.from(target.querySelectorAll<HTMLButtonElement>("button")).filter(
      (b) => b.getAttribute("aria-expanded") !== null,
    );
    for (const b of rules) b.click();
    flushSync();
    expect(row("tools/notes-app")?.textContent).toContain("Its own repository, branch develop.");
    expect(row("hubs/hub-b/projects/spec")?.textContent).toContain("No git repository.");
  });

  it("Checking git… per row while held; git missing: no branch, one sentence in the intro", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    const probe = mock.probe.plugins.gitMembers;
    probe.hold(true);
    mountHere(ProjectsSection);
    await until(
      () => row("tools/notes-app")?.textContent?.includes("Checking git…") === true,
      "checking",
    );
    probe.hold(false);
    probe.unavailable(true);
    await until(() => target.querySelector("[data-projects-note]") !== null, "note");
    expect(target.querySelectorAll("[data-projects-note]")).toHaveLength(1);
    expect(target.querySelector("[data-projects-note]")?.textContent).toBe("Git isn't available");
    expect(row("tools/notes-app")?.textContent).not.toContain("develop");
  });
});

describe("adversarial: many projects, odd repositories", () => {
  it("36 projects, all dirty, mid-rebase | detached | long | Unicode branches: every row and node says it, cut", async () => {
    await openFolder(MOCK_PROJECTS_MANY);
    const long = `feature/${"very-long-branch-name-".repeat(8)}end`;
    mock.probe.plugins.gitMembers.shape((s: RepoSummary, n: number): RepoSummary => {
      const kind = n % 4;
      return {
        ...s,
        error: null,
        changed: 5000,
        state: kind === 0 ? "rebasing" : "clean",
        detached: kind === 0 || kind === 3,
        branch: kind === 1 ? long : kind === 2 ? "功能/数据-ünïcödé ✨" : null,
      };
    });
    const { store } = await explorer();
    await expand(store, "repos");
    await until(() => target.querySelectorAll(".fact").length > 5, "facts drawn");
    const facts = Array.from(target.querySelectorAll<HTMLElement>(".fact"));
    expect(facts.some((f) => f.textContent === "REBASING" && f.classList.contains("chip"))).toBe(
      true,
    );
    expect(facts.some((f) => /^detached [0-9a-f]{7}$/.test(f.textContent ?? ""))).toBe(true);
    expect(facts.some((f) => f.textContent === long && f.title === long)).toBe(true);
    expect(facts.some((f) => f.textContent === "功能/数据-ünïcödé ✨")).toBe(true);
    // the folder dot of every drawn project node
    const nodes = Array.from(target.querySelectorAll<HTMLElement>("[data-project]"));
    expect(nodes.length).toBeGreaterThan(5);
    mountHere(ProjectsSection);
    await until(() => (target.textContent ?? "").includes("5000+ changed"), "cap in rows");
    // the backend takes at most 500 ids: what is sent stays within
    expect((interest("glance") ?? []).length).toBeLessThanOrEqual(500);
  });
});

describe("the status item", () => {
  it("follows the active document's project: its short name first; the opened folder's own: no prefix", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    mountHere(GitStatus);
    cleanups.push(() => void documents.show(null));
    await documents.show("tools/notes-app/README.md");
    await until(() => target.querySelector(".prefix")?.textContent === "notes-app ·", "prefix");
    expect(target.querySelector(".branch")?.textContent).toBe("develop");
    await until(() => (target.textContent ?? "").includes("3 changed"), "count");
    await until(() => (interest("glance") ?? []).includes(NOTES), "status asks its count");
    await documents.show("hubs/hub-b/README.md");
    await until(() => target.querySelector(".prefix")?.textContent === "hub-b ·", "hub-b");
    await documents.show("README.md");
    await until(() => target.querySelector(".prefix") === null, "root again");
  });
});

describe("the move question (a project renamed while PerMark was closed)", () => {
  it("asks 'Is this … ?' under the list; Yes answers with the found folder, focus back on the list", async () => {
    await openFolder(MOCK_PROJECTS_LISTED);
    await explorer();
    mock.probe.projects.askMove({
      uid: "hubx2hubx2",
      name: "hub-x",
      was: "hubs/hub-x",
      candidate: "tools/new-app",
    });
    await until(() => projects.strip.kind === "move", "move question");
    flushSync();
    const strip = target.querySelector<HTMLElement>(
      '[role="region"][aria-label="Projects in this folder"]',
    );
    expect(strip?.textContent).toContain("Is this hub-x, which was at hubs/hub-x?");
    const yes = Array.from(strip!.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Yes, it moved",
    )!;
    yes.click();
    await until(
      () =>
        mock.probe.projects.calls.includes("projects_answer_move hubx2hubx2 tools/new-app true"),
      "answered",
    );
    await until(() => projects.strip.kind !== "move", "question gone");
    await until(
      () => document.activeElement === target.querySelector('[role="tree"]'),
      "focus list",
    );
    expect(ProjectStrip).toBeDefined();
  });
});
