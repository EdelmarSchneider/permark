// @vitest-environment jsdom
// The repositories of included projects in the frontend (`projects.md` §8, §12): summaries, interest sent only when it
// changes (and `[]` when the view hides | goes), lists kept only for what is looked at, badges inside projects.
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createMockApi } from "../../api/mock";
import { MOCK_PROJECTS_LISTED, MOCK_PROJECTS_MANY } from "../../api/mockProjects";
import { PluginClient } from "../../core/plugins/client";
import { silence } from "../../core/silence/silence.svelte";
import { INTEREST_MS, GitMembers } from "./members.svelte";
import { GitStore } from "./store.svelte";
import { WindowParts } from "./testing.svelte";

const HUB = "k7m2q7xd2a";
const NOTES = "notes2note";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 6; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

type Interest = { ids: string[]; level: string; view?: string };

async function setup(root: string = MOCK_PROJECTS_LISTED) {
  const api = createMockApi();
  await api.openPath(root, "thisTab");
  await api.projectsView();
  const calls: Interest[] = [];
  const call = api.pluginCall.bind(api);
  vi.spyOn(api, "pluginCall").mockImplementation(((method: string, args: unknown) => {
    if (method === "git.interest") calls.push(structuredClone(args) as Interest);
    return call(method as never, args as never);
  }) as typeof api.pluginCall);
  const state = new WindowParts(root);
  const members = new GitMembers({
    client: new PluginClient(api, () => state.root),
    root: () => state.root,
    looking: () => state.looking,
    document: () => state.document,
    view: () => state.view,
    shown: () => state.shown,
  });
  const stop = members.start();
  await settle();
  return { api, members, state, calls, stop };
}

/** Lets the interest settle and the mock answer. */
async function answered(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, INTEREST_MS + 60));
  await settle();
}

afterEach(() => {
  vi.useRealTimers();
  silence.set({});
});

describe("the repositories of included projects", () => {
  it("knows every included git project at once (cheap facts), no interest sent while nothing is shown", async () => {
    const { members, calls, stop } = await setup();
    expect([...members.repos.keys()].sort()).toEqual([HUB, NOTES]);
    expect(members.of(NOTES)?.branch).toBe("develop");
    await answered();
    expect(calls).toEqual([]);
    stop();
  });

  it("sends what the window shows once it settles, then only what changed", async () => {
    const { state, calls, stop } = await setup();
    state.looking = { glance: [NOTES], detail: [] };
    flushSync();
    state.looking = { glance: [NOTES, "notaproject"], detail: [] };
    flushSync();
    await answered();
    // unknown ids never leave; one call for the settled set
    expect(calls).toEqual([{ ids: [NOTES], level: "glance", view: "7" }]);
    state.looking = { glance: [NOTES], detail: [NOTES] };
    flushSync();
    await answered();
    expect(calls.at(-1)).toEqual({ ids: [NOTES], level: "detail", view: "7" });
    expect(calls).toHaveLength(2);
    stop();
  });

  it("the active document's project is looked at (status bar count)", async () => {
    const { members, state, calls, stop } = await setup();
    state.document = "hubs/hub-b/README.md";
    flushSync();
    expect(members.active?.id).toBe(HUB);
    await answered();
    expect(calls).toEqual([{ ids: [HUB], level: "glance", view: "7" }]);
    state.document = "README.md";
    flushSync();
    expect(members.active).toBeNull();
    await answered();
    expect(calls.at(-1)).toEqual({ ids: [], level: "glance", view: "7" });
    stop();
  });

  it("a hidden tab says [] for both levels; shown again, its sets come back", async () => {
    const { state, calls, stop } = await setup();
    state.looking = { glance: [HUB], detail: [NOTES] };
    flushSync();
    await answered();
    calls.length = 0;
    state.shown = false;
    flushSync();
    await answered();
    expect(calls).toEqual([
      { ids: [], level: "glance", view: "7" },
      { ids: [], level: "detail", view: "7" },
    ]);
    state.shown = true;
    flushSync();
    await answered();
    expect(calls.slice(2)).toEqual([
      { ids: [HUB], level: "glance", view: "7" },
      { ids: [NOTES], level: "detail", view: "7" },
    ]);
    stop();
  });

  it("the view going drops its sets; a new view id drops the old view's first", async () => {
    const { state, calls, stop } = await setup();
    state.looking = { glance: [HUB], detail: [] };
    flushSync();
    await answered();
    state.view = "8";
    flushSync();
    await answered();
    expect(calls.slice(1)).toEqual([
      { ids: [], level: "glance", view: "7" },
      { ids: [HUB], level: "glance", view: "8" },
    ]);
    calls.length = 0;
    stop();
    expect(calls).toEqual([{ ids: [], level: "glance", view: "8" }]);
  });

  it("rapid looks (scrolling, 50 toggles) send one settled set", async () => {
    const { state, calls, stop } = await setup();
    for (let n = 0; n < 50; n++) {
      state.looking = { glance: n % 2 === 0 ? [HUB] : [HUB, NOTES], detail: [] };
      flushSync();
    }
    await answered();
    expect(calls).toEqual([{ ids: [HUB, NOTES], level: "glance", view: "7" }]);
    stop();
  });

  it("keeps the changed files of what is looked at, drops them when nobody looks", async () => {
    const { members, state, stop } = await setup();
    state.looking = { glance: [NOTES], detail: [] };
    flushSync();
    await answered();
    await answered();
    expect(members.lists.get(NOTES)?.files.length).toBe(3);
    expect(members.fileOf("tools/notes-app/README.md")?.file.letter).toBe("modified");
    state.looking = { glance: [], detail: [] };
    flushSync();
    await answered();
    expect(members.lists.size).toBe(0);
    expect(members.fileOf("tools/notes-app/README.md")).toBeUndefined();
    stop();
  });

  it("git missing: every repository says so (no branch anywhere)", async () => {
    const { api, members, stop } = await setup();
    api.probe.plugins.gitMembers.unavailable(true);
    await settle();
    expect([...members.repos.values()].every((r) => r.error?.kind === "unavailable")).toBe(true);
    expect([...members.repos.values()].every((r) => r.branch === null)).toBe(true);
    stop();
  });

  it("removed projects leave; a plain folder (no root) holds nothing", async () => {
    const { members, state, stop } = await setup();
    state.root = null;
    flushSync();
    await settle();
    expect(members.repos.size).toBe(0);
    stop();
  });
});

describe("badges inside included projects", () => {
  it("a letter on files of the project, a dot on its folders, the project folder counted from its summary", async () => {
    const { api, members, state, stop } = await setup();
    const store = new GitStore(new PluginClient(api, () => state.root), () => state.root, members);
    const stopStore = store.start();
    await settle();
    // not looked at: the folder counts from the summary, no letters
    expect(store.badge("tools/notes-app", true)).toMatchObject({
      text: "•",
      title: "3 changed files inside",
    });
    expect(store.badge("tools/notes-app/README.md", false)).toBeNull();
    // hub-b is clean
    expect(store.badge("hubs/hub-b", true)).toBeNull();
    state.looking = { glance: [NOTES], detail: [] };
    flushSync();
    await answered();
    await answered();
    expect(store.badge("tools/notes-app/README.md", false)).toMatchObject({ text: "M" });
    expect(store.badge("tools/notes-app/docs", true)).toMatchObject({ text: "•" });
    expect(store.badge("tools/notes-app", true)?.title).toBe("3 changed files inside");
    // the folder above the project is the opened folder's: project files never count there
    expect(store.changedBelow("tools")).toBe(0);
    // silenced: nothing drawn, nothing counted
    silence.set({ "tools/notes-app/docs": "quiet" });
    flushSync();
    expect(store.badge("tools/notes-app/docs/note-1.md", false)).toBeNull();
    expect(store.badge("tools/notes-app", true)?.title).toBe("1 changed file inside");
    stopStore();
    stop();
  });

  it("40 projects all dirty: every folder has its dot; git missing: none", async () => {
    const { api, members, state, stop } = await setup(MOCK_PROJECTS_MANY);
    api.probe.plugins.gitMembers.shape((s) => ({ ...s, changed: 4, error: null }));
    await settle();
    const store = new GitStore(new PluginClient(api, () => state.root), () => state.root, members);
    const stopStore = store.start();
    await settle();
    const repos = [...members.repos.values()];
    expect(repos.length).toBeGreaterThanOrEqual(30);
    expect(repos.every((r) => store.badge(r.path, true)?.text === "•")).toBe(true);
    api.probe.plugins.gitMembers.unavailable(true);
    await settle();
    expect(repos.some((r) => store.badge(r.path, true) !== null)).toBe(false);
    stopStore();
    stop();
  });
});
