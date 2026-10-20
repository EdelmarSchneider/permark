// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { GitSnapshot } from "../../api";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { PluginClient } from "../../core/plugins/client";
import { silence } from "../../core/silence/silence.svelte";
import { GitStore, letterOf } from "./store.svelte";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 5; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

async function setup(root: string | null = MOCK_FOLDER) {
  const api = createMockApi();
  await api.openPath(MOCK_FOLDER, "thisTab");
  const store = new GitStore(new PluginClient(api, () => root), () => root);
  const stop = store.start();
  await settle();
  return { api, store, stop };
}

function snapshot(patch: Partial<GitSnapshot>): GitSnapshot {
  return {
    seq: 1,
    backend: "builtin",
    head: { branch: "main", oid: "a".repeat(40), detached: false, unborn: false },
    state: "clean",
    baseline: { kind: "head", oid: null },
    files: [],
    truncated: false,
    worktrees: [],
    ...patch,
  };
}

afterEach(() => vi.useRealTimers());

describe("the git store", () => {
  it("loads the snapshot and answers the rows: a letter on files, a dot with a count on folders", async () => {
    const { store, stop } = await setup();
    expect(store.state.status).toBe("ready");
    expect(store.badge("docs/guide.md", false)).toMatchObject({ text: "M", tone: "modified" });
    expect(store.badge("notes/ideas.md", false)).toMatchObject({ text: "U", tone: "untracked" });
    expect(store.badge("README.md", false)).toBeNull();
    expect(store.badge("docs", true)).toMatchObject({ text: "•", tone: "modified" });
    expect(store.badge("src", true)).toBeNull();
    expect(store.changedBelow("notes")).toBe(1);
    expect(store.changed).toBe(2);
    stop();
  });

  it("draws no letter or dot for what the person silenced, and counts only the rest", async () => {
    const { store, stop } = await setup();
    silence.set({ notes: "quiet" });
    expect(store.badge("notes/ideas.md", false)).toBeNull();
    expect(store.badge("notes", true)).toBeNull();
    expect(store.badge("docs/guide.md", false)).toMatchObject({ text: "M" });
    expect(store.changedBelow("notes")).toBe(0);
    expect(store.changedBelow("docs")).toBe(1);
    silence.clear();
    expect(store.badge("notes/ideas.md", false)).toMatchObject({ text: "U" });
    stop();
  });

  it("follows the announcements and ignores an older picture arriving late", async () => {
    const { store, api, stop } = await setup();
    const seq = store.snapshot?.seq ?? 0;
    api.probe.plugins.announceGit(
      snapshot({
        seq: seq + 4,
        files: [{ path: "a/b/c.md", letter: "deleted", staged: true, from: null }],
      }),
    );
    expect(store.badge("a/b/c.md", false)).toMatchObject({ text: "D", tone: "deleted" });
    expect(store.badge("a", true)).not.toBeNull();
    expect(store.badge("a/b", true)).not.toBeNull();
    expect(store.badge("docs/guide.md", false)).toBeNull();
    // an older one arriving late changes nothing
    api.probe.plugins.announceGit(snapshot({ seq: seq + 1, files: [] }));
    expect(store.badge("a/b/c.md", false)).not.toBeNull();
    // the repository is gone
    api.probe.plugins.announceGit(null);
    expect(store.state.status).toBe("none");
    expect(store.badge("a/b/c.md", false)).toBeNull();
    stop();
  });

  it("is off without a project: no badge, nothing asked", async () => {
    const { store, stop, api } = await setup(null);
    expect(store.state.status).toBe("off");
    expect(store.badge("docs/guide.md", false)).toBeNull();
    expect(api.probe.plugins.calls).toEqual([]);
    stop();
  });

  it("the baseline switch and a refresh reach the backend", async () => {
    const { store, api, stop } = await setup();
    await store.setBaseline("head");
    await settle();
    expect(api.probe.plugins.calls).toContain("git.setBaseline");
    expect(store.snapshot?.baseline.kind).toBe("head");
    await store.refresh();
    expect(api.probe.plugins.calls).toContain("git.refresh");
    stop();
  });

  it("a window that gets the focus again looks at the repository again", async () => {
    const { api, stop } = await setup();
    const before = api.probe.plugins.calls.filter((c) => c === "git.refresh").length;
    window.dispatchEvent(new Event("focus"));
    await settle();
    expect(api.probe.plugins.calls.filter((c) => c === "git.refresh").length).toBe(before + 1);
    stop();
  });

  it("explains each letter in plain words, with what it is measured from", async () => {
    const { store, stop } = await setup();
    const file = { path: "x.md", letter: "renamed" as const, staged: true, from: "old.md" };
    const text = store.explain(file);
    expect(text).toContain("old.md");
    expect(text.toLowerCase()).toContain("renamed");
    expect(["M", "A", "U", "D", "R", "C"].sort()).toEqual(
      (["modified", "added", "untracked", "deleted", "renamed", "conflict"] as const)
        .map(letterOf)
        .sort(),
    );
    stop();
  });

  it("teardown leaves it off with nothing listed", async () => {
    const { store, stop } = await setup();
    stop();
    expect(store.state.status).toBe("off");
    expect(store.badge("docs/guide.md", false)).toBeNull();
    expect(store.changed).toBe(0);
  });
});
