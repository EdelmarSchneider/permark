// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { describe, expect, it } from "vitest";
import type { ActivityEntry, ActivityFact } from "../../api";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { t } from "../../core/i18n";
import { PluginClient } from "../../core/plugins/client";
import { mineLabel, panelRows, pathOf } from "./mine";
import { FeedStore } from "./store.svelte";
import { entry } from "./testing";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 5; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

const act = (seq: number, fact: ActivityFact, at = 5000 + seq): ActivityEntry => ({
  seq,
  at,
  by: "person",
  fact,
});

describe("the person's actions", () => {
  it("say what was done, with the sections, the section or where a result went", () => {
    const say = (fact: ActivityFact): string | null => {
      const label = mineLabel(fact);
      return label === null
        ? null
        : `${t(label.key, label.params)}${label.detail ? ` · ${label.detail}` : ""}${label.undone ? " (undo)" : ""}`;
    };
    expect(say({ kind: "save", path: "a.md", sections: ["Plan > Goals", "Plan"], blocks: 2 })).toBe(
      "Saved · Plan › Goals · Plan",
    );
    expect(say({ kind: "file", op: "move", path: "b/a.md", from: "a.md", undo: true })).toBe(
      "Moved from a.md (undo)",
    );
    expect(say({ kind: "file", op: "new", path: "n.md" })).toBe("Created");
    expect(say({ kind: "convert", path: "d.json", to: "toon", saved: "d.toon" })).toBe(
      "Converted to TOON · d.toon",
    );
    expect(say({ kind: "comment", op: "done", path: "a.md", id: "c1", heading: "A > B" })).toBe(
      "Comment marked done · A › B",
    );
    expect(say({ kind: "tag", added: false, path: "a.md", tag: "plan" })).toBe("Tag plan removed");
    expect(say({ kind: "review", op: "revert", path: "a.md", section: null })).toBe(
      "Change reverted",
    );
    expect(say({ kind: "outside", change: "modified", path: "a.md" })).toBeNull();
    expect(say({ kind: "handOff", rows: 3 })).toBeNull();
    expect(say({ kind: "closed" })).toBeNull();
    expect(pathOf({ kind: "handOff", rows: 3 })).toBeNull();
  });

  it("the filter shows mine | outside | all, the person's disk changes only once", () => {
    const feed = [
      entry(1, "agent.md", "modified", { atMs: 100 }),
      entry(2, "mine.md", "modified", { atMs: 200, own: true }),
    ];
    const mine = [
      act(1, { kind: "save", path: "mine.md", sections: ["A"], blocks: 1 }, 210),
      act(2, { kind: "handOff", rows: 1 }, 300),
      act(3, { kind: "closed" }, 400),
    ];
    const keys = (show: "all" | "mine" | "outside"): string[] =>
      panelRows(feed, mine, show).map((r) => r.key);
    expect(keys("all")).toEqual(["m2", "m1", "f1"]);
    expect(keys("mine")).toEqual(["m2", "m1"]);
    expect(keys("outside")).toEqual(["f1"]);
  });

  it("come from the journal at start and as they happen; Docs only applies to them too", async () => {
    const api = createMockApi();
    await api.openPath(MOCK_FOLDER, "thisTab");
    const store = new FeedStore(
      new PluginClient(api, () => MOCK_FOLDER),
      () => MOCK_FOLDER,
      () => null,
    );
    const stop = store.start();
    await settle();
    expect(store.mine.map((e) => e.seq)).toEqual([1, 2, 3, 4]);
    api.probe.plugins.mine(
      act(9, { kind: "tag", added: true, path: "data.json", tag: "x" }, Date.now()),
    );
    await settle();
    expect(store.mine.at(-1)?.seq).toBe(9);
    store.show = "mine";
    expect(store.rows[0]?.key).toBe("m9");
    store.docsOnly = true;
    expect(store.rows.some((r) => r.key === "m9")).toBe(false);
    expect(
      store.rows.some((r) => r.key === "m2"),
      "a hand-off mark stays",
    ).toBe(true);
    // the same entry twice is listed once
    store.takeMine(act(9, { kind: "tag", added: true, path: "data.json", tag: "x" }));
    expect(store.mine.filter((e) => e.seq === 9)).toHaveLength(1);
    stop();
  });
});

describe("project rows", () => {
  it("are news from outside: listed in all + outside, never in mine", () => {
    const news = {
      item: "project" as const,
      id: 7,
      atMs: 9000,
      event: { kind: "appeared" as const, name: "notes-app", path: "tools" },
    };
    const feed = [entry(1, "a.md"), news];
    expect(panelRows(feed, [], "all").map((r) => r.key)).toEqual(["f7", "f1"]);
    expect(panelRows(feed, [], "outside").map((r) => r.key)).toEqual(["f7", "f1"]);
    expect(panelRows(feed, [], "mine")).toEqual([]);
  });
});
