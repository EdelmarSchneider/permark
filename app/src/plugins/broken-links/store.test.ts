// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { describe, expect, it } from "vitest";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { PluginClient } from "../../core/plugins/client";
import { BrokenLinksStore, RELOAD_MS } from "./store.svelte";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 5; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

async function setup() {
  const api = createMockApi();
  await api.openPath(MOCK_FOLDER, "thisTab");
  const store = new BrokenLinksStore(new PluginClient(api, () => MOCK_FOLDER), () => MOCK_FOLDER);
  const stop = store.start();
  await settle();
  return { api, store, stop };
}

describe("the broken links store", () => {
  it("is quiet while the demo project has none", async () => {
    const { store, stop } = await setup();
    expect(store.report?.ready).toBe(true);
    expect(store.current).toBe(0);
    stop();
  });

  it("lists a document's missing links with their lines", async () => {
    const { api, store, stop } = await setup();
    api.probe.project.add(
      "docs/bad.md",
      "# Bad\n\n[ok](guide.md) [web](https://example.com) [anchor](#top)\n\n[gone](missing.md) and ![pic](img/none.png)\n",
    );
    await store.load();
    expect(store.current).toBe(2);
    expect(store.report?.docs).toEqual([
      {
        path: "docs/bad.md",
        links: [
          { line: 5, written: "missing.md" },
          { line: 5, written: "img/none.png" },
        ],
      },
    ]);
    stop();
  });

  it("counts the opened folder apart from its linked worktrees, and lists it first (#62)", async () => {
    const { api, store, stop } = await setup();
    api.probe.project.add("docs/bad.md", "[gone](missing.md)\n");
    api.probe.project.add("@wt/feature/docs/bad.md", "[a](gone-a.md) [b](gone-b.md)\n");
    await store.load();
    expect(store.report?.total).toBe(3);
    expect(store.report?.current).toBe(1);
    expect(store.report?.roots).toEqual([
      { prefix: "", total: 1 },
      { prefix: "@wt/feature", total: 2 },
    ]);
    expect(store.report?.docs.map((d) => d.path)).toEqual([
      "docs/bad.md",
      "@wt/feature/docs/bad.md",
    ]);
    expect(store.current).toBe(1);
    stop();
  });

  it("answers like the backend: worktrees in the vault's order, each one listed also at 0, others after", async () => {
    const { api, store, stop } = await setup();
    api.probe.project.worktrees.push("zeta", "clean", "alpha");
    api.probe.project.add("docs/bad.md", "[gone](missing.md)\n");
    api.probe.project.add("@wt/alpha/a.md", "[a](gone.md)\n");
    api.probe.project.add("@wt/zeta/z.md", "[z](gone.md)\n");
    api.probe.project.add("@wt/other/o.md", "[o](gone.md)\n");
    await store.load();
    expect(store.report?.roots).toEqual([
      { prefix: "", total: 1 },
      { prefix: "@wt/zeta", total: 1 },
      { prefix: "@wt/clean", total: 0 },
      { prefix: "@wt/alpha", total: 1 },
      { prefix: "@wt/other", total: 1 },
    ]);
    expect(store.report?.docs.map((d) => d.path)).toEqual([
      "docs/bad.md",
      "@wt/zeta/z.md",
      "@wt/alpha/a.md",
      "@wt/other/o.md",
    ]);
    stop();
  });

  it("cuts the list at 500 links like the backend, a worktree's before the opened folder's", async () => {
    const { api, store, stop } = await setup();
    api.probe.project.worktrees.push("big");
    const many = Array.from({ length: 600 }, (_, i) => `[l${i}](gone-${i}.md)`).join("\n");
    api.probe.project.add("@wt/big/aaa.md", `${many}\n`);
    api.probe.project.add("zzz.md", "[a](gone-a.md)\n\n[b](gone-b.md)\n");
    await store.load();
    const report = store.report;
    expect(report?.docs[0]?.path).toBe("zzz.md");
    expect(report?.docs.reduce((n, d) => n + d.links.length, 0)).toBe(500);
    expect(report?.truncated).toBe(true);
    expect(report?.total).toBe(602);
    expect(report?.current).toBe(2);
    expect(report?.roots).toEqual([
      { prefix: "", total: 2 },
      { prefix: "@wt/big", total: 600 },
    ]);
    stop();
  });

  it("asks again when the backend says the picture changed", async () => {
    const { api, store, stop } = await setup();
    api.probe.project.add("a.md", "[x](nope.md)\n");
    // the mock backend announces nothing for a broken link; the store reloads on the event
    api.probe.plugins.announce("broken-links:changed", { ready: true, total: 1, current: 1 });
    await new Promise((resolve) => setTimeout(resolve, RELOAD_MS + 30));
    await settle();
    expect(store.current).toBe(1);
    stop();
  });

  it("forgets the list when the project goes", async () => {
    const { api, store, stop } = await setup();
    api.probe.project.add("a.md", "[x](nope.md)\n");
    await store.load();
    expect(store.current).toBe(1);
    stop();
    expect(store.report).toBeNull();
  });
});
