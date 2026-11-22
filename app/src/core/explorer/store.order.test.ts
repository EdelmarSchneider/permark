// @vitest-environment jsdom
// The Explorer's rows as the panel draws them (in an effect): a delta re-places only what it brings, an unchanged
// list told again draws nothing anew.
import { flushSync } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { observe } from "../testing/effect.svelte";
import { inNameOrder } from "./list";
import { ExplorerStore } from "./store.svelte";
import { language } from "../i18n";

afterEach(() => {
  vi.restoreAllMocks();
});

it("a delta places what it brings without sorting the whole list again; an empty one keeps the rows", async () => {
  const api = createMockApi();
  await api.openPath(MOCK_FOLDER, "thisTab");
  const store = new ExplorerStore(api, 1, {
    fail: () => {},
    note: () => {},
    view: () => "main",
    linkUpdate: () => "ask",
    openProject: () => {},
  });
  const stop = await store.start();
  const drawn = observe(() => void store.rows);
  flushSync();
  // a full sort makes a collator; the kept order makes none
  const collators = vi.spyOn(Intl, "Collator");
  try {
    await api.newEntry("", "File 0", "file");
    await api.newEntry("docs", "a b", "folder");
    flushSync();
    expect(collators).not.toHaveBeenCalled();
    const shown = store.rows.flatMap((r) => (r.type === "node" ? [r.node.path] : []));
    expect(shown).toContain("File 0.md");
    collators.mockRestore();
    const sorted = inNameOrder(store.nodes, language.locale).map((n) => n.path);
    expect(shown).toEqual(sorted.filter((p) => shown.includes(p)));
    const runs = drawn.runs();
    const nodes = store.nodes;
    // the folder read again with nothing changed: the backend still tells the list, as an empty delta
    await api.treeRefresh();
    flushSync();
    expect(store.nodes).toBe(nodes);
    expect(drawn.runs()).toBe(runs);
  } finally {
    drawn.stop();
    stop();
  }
});
