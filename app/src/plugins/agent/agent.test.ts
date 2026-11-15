// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AgentWindowContext, DocumentView } from "../../api";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import type { OutlineItem } from "../../api/generated/OutlineItem";
import { documents } from "../../core/doc/document.svelte";
import { editing } from "../../core/editing/session.svelte";
import { PluginClient } from "../../core/plugins/client";
import {
  ContributionRegistry,
  PluginHost,
  pluginLoaders,
} from "../../core/plugins/registry.svelte";
import { Box } from "../../core/testing/box.svelte";
import { windowStore } from "../../core/window.svelte";
import { sameContext, sectionOf } from "./context";
import { AgentStore, PUBLISH_DELAY_MS, showPlace, type Place } from "./store.svelte";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 5; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

const item = (label: string, level: number, line: number): OutlineItem => ({
  label,
  level,
  id: label.toLowerCase(),
  line,
  block: `b${String(line)}`,
});

function viewing(doc: string | null): AgentWindowContext {
  return { doc, mode: "view", section: null, selection: null, unsaved: false };
}

async function setup() {
  const api = createMockApi();
  await api.openPath(MOCK_FOLDER, "thisTab");
  const context = new Box<AgentWindowContext>(viewing("README.md"));
  const shown: Place[] = [];
  const store = new AgentStore(
    new PluginClient(api, () => MOCK_FOLDER),
    () => MOCK_FOLDER,
    () => context.value,
    (place) => shown.push(place),
  );
  const stop = store.start();
  await settle();
  return { api, store, stop, context, shown };
}

afterEach(() => {
  vi.restoreAllMocks();
  editing.mode = "view";
});

describe("the section the person is in", () => {
  it("is the heading path above the current heading, with its lines", () => {
    const outline = [
      item("Plan", 1, 1),
      item("Rollout", 2, 5),
      item("Steps", 3, 9),
      item("Risks", 2, 14),
    ];
    expect(sectionOf(outline, 2)).toEqual({
      headingPath: ["Plan", "Rollout", "Steps"],
      lines: [9, 13],
    });
    expect(sectionOf(outline, 1)).toEqual({ headingPath: ["Plan", "Rollout"], lines: [5, 13] });
    expect(sectionOf(outline, 3)).toEqual({ headingPath: ["Plan", "Risks"], lines: null });
    expect(sectionOf(outline, -1)).toBeNull();
    expect(sectionOf([], 0)).toBeNull();
    expect(sameContext(null, viewing("a.md"))).toBe(false);
    expect(sameContext(viewing("a.md"), viewing("a.md"))).toBe(true);
  });
});

describe("the window's own context", () => {
  it("names the document shown, and no section of the previous page while it loads", async () => {
    const api = createMockApi();
    await api.openPath(MOCK_FOLDER, "thisTab");
    const previous = {
      path: "README.md",
      body: { kind: "markdown", blocks: [], outline: [item("Intro", 1, 1)], frontMatter: null },
    } as unknown as DocumentView;
    vi.spyOn(documents, "path", "get").mockReturnValue("docs/guide.md");
    vi.spyOn(documents, "view", "get").mockReturnValue(previous);
    documents.currentHeading = 0;
    const store = new AgentStore(new PluginClient(api, () => MOCK_FOLDER), () => MOCK_FOLDER);
    const stop = store.start();
    try {
      await settle();
      await store.setEnabled(true);
      await settle();
      await new Promise((resolve) => setTimeout(resolve, PUBLISH_DELAY_MS + 50));
      expect(api.probe.plugins.agent.published.at(-1)).toEqual(viewing("docs/guide.md"));
    } finally {
      stop();
      documents.currentHeading = -1;
      await store.setEnabled(false);
    }
  });
});

describe("the agent store", () => {
  it("publishes nothing while the switch is off, then what the person looks at, once per change", async () => {
    const { api, store, stop, context } = await setup();
    expect(store.enabled).toBe(false);
    await new Promise((resolve) => setTimeout(resolve, PUBLISH_DELAY_MS + 50));
    expect(api.probe.plugins.agent.published).toHaveLength(0);
    await store.setEnabled(true);
    await settle();
    expect(store.enabled).toBe(true);
    await new Promise((resolve) => setTimeout(resolve, PUBLISH_DELAY_MS + 50));
    expect(api.probe.plugins.agent.published).toEqual([viewing("README.md")]);
    // a burst of changes sends only the last one
    context.value = viewing("docs/a.md");
    flushSync();
    context.value = viewing("docs/b.md");
    flushSync();
    await new Promise((resolve) => setTimeout(resolve, PUBLISH_DELAY_MS + 50));
    expect(api.probe.plugins.agent.published.map((c) => c.doc)).toEqual(["README.md", "docs/b.md"]);
    // off again: dormant
    await store.setEnabled(false);
    await settle();
    context.value = viewing("docs/c.md");
    flushSync();
    await new Promise((resolve) => setTimeout(resolve, PUBLISH_DELAY_MS + 50));
    expect(api.probe.plugins.agent.published).toHaveLength(2);
    stop();
  });

  it("follows the backend's status: a client connects, requests come in", async () => {
    const { api, store, stop } = await setup();
    await store.setEnabled(true);
    api.probe.plugins.agent.connect("Claude Code");
    api.probe.plugins.agent.request({
      session: "mocksession01",
      tool: "search",
      subject: "retry",
      atMs: 1,
      outcome: "ok",
    });
    await settle();
    expect(store.connected).toEqual(["Claude Code"]);
    expect(store.status?.recent[0]?.tool).toBe("search");
    stop();
  });

  it("hands a place an agent asks for to the window, and offers resolving once asked", async () => {
    const { api, stop, shown } = await setup();
    const place = { path: "docs/guide.md", line: 3, heading: null, window: null };
    api.probe.plugins.announce("agent:open", place);
    await settle();
    expect(shown).toEqual([place]);
    const before = windowStore.notices.length;
    api.probe.plugins.announce("agent:ask", { group: "comments.resolve" });
    await settle();
    expect(windowStore.notices.length).toBe(before + 1);
    stop();
  });

  it("shows a place only when it is meant for this window (or for every window)", async () => {
    const label = windowStore.label;
    windowStore.label = "view-1";
    const { api, stop, shown } = await setup();
    try {
      const at = (window: string | null): Place => ({
        path: "a.md",
        line: 1,
        heading: null,
        window,
      });
      api.probe.plugins.announce("agent:open", at("view-2"));
      api.probe.plugins.announce("agent:open", at("view-1"));
      api.probe.plugins.announce("agent:open", at(null));
      await settle();
      expect(shown.map((p) => p.window)).toEqual(["view-1", null]);
    } finally {
      stop();
      windowStore.label = label;
    }
  });

  it("tells the backend when the person turns to the window, only while the switch is on", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    const { api, store, stop } = await setup();
    window.dispatchEvent(new Event("focus"));
    expect(api.probe.plugins.agent.focused).toBe(0);
    await store.setEnabled(true);
    await settle();
    expect(api.probe.plugins.agent.focused).toBe(1);
    window.dispatchEvent(new Event("focus"));
    await settle();
    expect(api.probe.plugins.agent.focused).toBe(2);
    await store.setEnabled(false);
    await settle();
    window.dispatchEvent(new Event("focus"));
    await settle();
    expect(api.probe.plugins.agent.focused).toBe(2);
    stop();
  });
});

describe("notes shared with the project", () => {
  const READING = "2026-10-16 1031.md";
  const TRIP = "2026-10-19 0907.md";

  it("reads what is shared and what can be added, newest first; sharing and stopping update both", async () => {
    const { store, stop } = await setup();
    expect(store.sharedNotes).toBeNull();
    await store.refreshNotes();
    expect(store.sharedNotes?.shared.map((n) => n.id)).toEqual([READING]);
    const candidates = store.sharedNotes?.candidates ?? [];
    expect(candidates[0]?.id).toBe(TRIP);
    expect(candidates.map((n) => n.id)).not.toContain(READING);
    expect(await store.shareNote(TRIP)).toBe(true);
    expect(store.sharedNotes?.shared.map((n) => n.id)).toEqual([TRIP, READING]);
    expect(store.sharedNotes?.candidates.map((n) => n.id)).not.toContain(TRIP);
    expect(await store.unshareNote(READING)).toBe(true);
    expect(store.sharedNotes?.shared.map((n) => n.id)).toEqual([TRIP]);
    expect(store.sharedNotes?.candidates.map((n) => n.id)).toContain(READING);
    stop();
  });

  it("knows nothing while the notes plugin is off (nothing is drawn), and says a refused change", async () => {
    const { api, store, stop } = await setup();
    await api.settingsSet({ key: "pluginEnabled", value: { plugin: "notes", enabled: false } });
    await store.refreshNotes();
    expect(store.sharedNotes).toBeNull();
    await api.settingsSet({ key: "pluginEnabled", value: { plugin: "notes", enabled: null } });
    await store.refreshNotes();
    expect(store.sharedNotes).not.toBeNull();
    const before = windowStore.notices.length;
    expect(await store.shareNote("gone.md")).toBe(false);
    expect(windowStore.notices.length).toBe(before + 1);
    stop();
  });

  it("drops an answer that arrives after the window turned to another project", async () => {
    const api = createMockApi();
    await api.openPath(MOCK_FOLDER, "thisTab");
    let root: string | null = MOCK_FOLDER;
    const store = new AgentStore(new PluginClient(api, () => root), () => root);
    // answers wait until the test lets them go
    const real = api.pluginCall.bind(api);
    const waiting: (() => void)[] = [];
    vi.spyOn(api, "pluginCall").mockImplementation((method, args) =>
      real(method, args).then(
        (answer) => new Promise((resolve) => waiting.push(() => resolve(answer))),
      ),
    );
    const letGo = async (): Promise<void> => {
      await vi.waitFor(() => expect(waiting.length).toBeGreaterThan(0));
      waiting.shift()?.();
    };
    const reading = store.refreshNotes();
    root = "/elsewhere";
    await letGo();
    await reading;
    expect(store.sharedNotes).toBeNull();
    root = MOCK_FOLDER;
    const sharing = store.shareNote(TRIP);
    root = "/elsewhere";
    await letGo();
    await sharing;
    expect(store.sharedNotes).toBeNull();
    // the same project: taken
    root = MOCK_FOLDER;
    const again = store.refreshNotes();
    await letGo();
    await again;
    expect(store.sharedNotes?.shared.map((n) => n.id)).toContain(TRIP);
  });
});

describe("showing an agent's place", () => {
  it("moves a reading person, never one who is editing", () => {
    const navigate = vi.spyOn(documents, "navigate").mockImplementation(() => undefined);
    showPlace({ path: "docs/guide.md", line: 7, heading: null, window: null });
    expect(navigate).toHaveBeenCalledWith("docs/guide.md", null, 7);
    editing.mode = "edit";
    const before = windowStore.notices.length;
    showPlace({ path: "docs/guide.md", line: 9, heading: null, window: null });
    expect(navigate).toHaveBeenCalledTimes(1);
    const notice = windowStore.notices[windowStore.notices.length - 1];
    expect(windowStore.notices.length).toBe(before + 1);
    expect(notice?.kind).toBe("action");
    if (notice?.kind === "action") {
      notice.actions[0]?.run();
      expect(navigate).toHaveBeenCalledTimes(2);
    }
  });
});

describe("the agent plugin", () => {
  it("adds its parts when switched on and leaves nothing when switched off", async () => {
    const registry = new ContributionRegistry();
    const host = new PluginHost(registry, pluginLoaders());
    await host.enable("agent");
    expect(host.state("agent")).toBe("on");
    const kinds = [...new Set(registry.ownedBy("agent").map((c) => c.kind))].sort();
    expect(kinds).toEqual([
      "command",
      "explorerNote",
      "projectSection",
      "settingsPage",
      "statusItem",
    ]);
    expect(host.disable("agent")).toBe(0);
    expect(registry.items).toHaveLength(0);
  });
});
