// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AgentWindowContext } from "../../api";
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
    api.probe.plugins.agent.request({ tool: "search", subject: "retry", atMs: 1, outcome: "ok" });
    await settle();
    expect(store.connected).toEqual(["Claude Code"]);
    expect(store.status?.recent[0]?.tool).toBe("search");
    stop();
  });

  it("hands a place an agent asks for to the window, and offers resolving once asked", async () => {
    const { api, stop, shown } = await setup();
    api.probe.plugins.announce("agent:open", { path: "docs/guide.md", line: 3, heading: null });
    await settle();
    expect(shown).toEqual([{ path: "docs/guide.md", line: 3, heading: null }]);
    const before = windowStore.notices.length;
    api.probe.plugins.announce("agent:ask", { group: "comments.resolve" });
    await settle();
    expect(windowStore.notices.length).toBe(before + 1);
    stop();
  });
});

describe("showing an agent's place", () => {
  it("moves a reading person, never one who is editing", () => {
    const navigate = vi.spyOn(documents, "navigate").mockImplementation(() => undefined);
    showPlace({ path: "docs/guide.md", line: 7, heading: null });
    expect(navigate).toHaveBeenCalledWith("docs/guide.md", null, 7);
    editing.mode = "edit";
    const before = windowStore.notices.length;
    showPlace({ path: "docs/guide.md", line: 9, heading: null });
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
    expect(kinds).toEqual(["command", "projectSection", "settingsSection", "statusItem"]);
    expect(host.disable("agent")).toBe(0);
    expect(registry.items).toHaveLength(0);
  });
});
