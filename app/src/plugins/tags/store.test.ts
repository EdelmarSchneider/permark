// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { PluginClient } from "../../core/plugins/client";
import { RELOAD_MS, TagsStore } from "./store.svelte";

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
  api.probe.project.add("fm.md", "---\ntitle: x\ntags: [alpha, Beta]\n---\n# T\n");
  const notes: string[] = [];
  const store = new TagsStore(
    new PluginClient(api, () => MOCK_FOLDER),
    () => MOCK_FOLDER,
    (text) => notes.push(text),
  );
  const stop = store.start();
  await settle();
  return { api, store, stop, notes };
}

async function later(ms = RELOAD_MS + 20): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, ms));
  await settle();
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("what the window shows", () => {
  it("reads the tags of the front matter and the counts", async () => {
    const { store, stop } = await setup();
    expect(store.tagsOf("fm.md")).toEqual(["alpha", "beta"]);
    expect(store.tagsOf("README.md")).toEqual([]);
    expect(store.overview?.tags.map((t) => t.name)).toEqual(["alpha", "beta"]);
    expect(store.overview?.untagged).toBeGreaterThan(0);
    stop();
  });

  it("follows an edit: tags, description, a line in the row tooltip", async () => {
    const { store, stop } = await setup();
    const meta = await store.setTags("docs/guide.md", ["Spec", "needs review"]);
    expect(meta?.tags.map((t) => t.name)).toEqual(["spec", "needs-review"]);
    await store.setDescription("docs/guide.md", "  How the guide\nreads ");
    await later();
    expect(store.tagsOf("docs/guide.md")).toEqual(["spec", "needs-review"]);
    expect(store.descriptionOf("docs/guide.md")).toBe("How the guide reads");
    expect(store.badge("docs/guide.md", false)).toEqual({
      text: "",
      tip: "Tags: spec, needs-review\nHow the guide reads",
    });
    expect(store.badge("README.md", false)).toBeNull();
    expect(store.badge("docs", true)).toBeNull();
    // a file with its tags taken away is quiet again
    await store.setTags("docs/guide.md", []);
    await store.setDescription("docs/guide.md", "");
    await later();
    expect(store.badge("docs/guide.md", false)).toBeNull();
    stop();
  });

  it("gives a tag its look", async () => {
    const { store, stop } = await setup();
    await store.setTags("README.md", ["spec"]);
    await store.setStyle("spec", "blue", "book-open");
    await later();
    expect(store.styleOf("spec")).toEqual({ color: "blue", icon: "book-open" });
    await store.setStyle("spec", "magenta", "rocket");
    await later();
    expect(store.styleOf("spec")).toBeUndefined();
    stop();
  });
});

describe("the tags view", () => {
  it("lists the files that have the selected tag, or all of several (Ctrl + click)", async () => {
    const { store, stop } = await setup();
    await store.setTags("README.md", ["alpha", "extra"]);
    await later();
    store.pick("alpha", false);
    await settle();
    expect(store.listing).toMatchObject({ status: "ready" });
    const files = (): string[] =>
      store.listing.status === "ready" ? store.listing.files.map((f) => f.path) : [];
    expect(files()).toEqual(["README.md", "fm.md"]);
    store.pick("extra", true);
    await settle();
    expect(store.selected).toEqual(["alpha", "extra"]);
    expect(files()).toEqual(["README.md"]);
    // Ctrl + click on a selected tag leaves the selection; a plain click on the only one clears it
    store.pick("extra", true);
    store.pick("alpha", false);
    await settle();
    expect(store.selected).toEqual([]);
    expect(store.listing).toEqual({ status: "idle" });
    stop();
  });

  it("lists the documents without any tag", async () => {
    const { store, stop } = await setup();
    store.pickUntagged();
    await settle();
    expect(store.untaggedOnly).toBe(true);
    const files = store.listing.status === "ready" ? store.listing.files.map((f) => f.path) : [];
    expect(files).toContain("README.md");
    expect(files).not.toContain("fm.md");
    store.pickUntagged();
    expect(store.untaggedOnly).toBe(false);
    expect(store.listing).toEqual({ status: "idle" });
    stop();
  });

  it("filters the list by the box", async () => {
    const { store, stop } = await setup();
    store.filter = "alp";
    expect(store.listed.map((t) => t.name)).toEqual(["alpha"]);
    store.filter = "";
    expect(store.listed).toHaveLength(2);
    stop();
  });
});

describe("a file that cannot be read", () => {
  it("refuses changes, says why, and the app's tags stay read-only", async () => {
    const { api, store, stop, notes } = await setup();
    api.probe.plugins.organizing.tagsProblem({ message: "unexpected token", line: 3 });
    await later();
    expect(store.status?.problem).toEqual({ message: "unexpected token", line: 3 });
    expect(await store.setTags("README.md", ["x"])).toBeNull();
    expect(notes).toHaveLength(1);
    expect(notes[0]).toContain("Couldn't change the tags");
    api.probe.plugins.organizing.tagsProblem(null);
    await later();
    expect(store.status?.problem).toBeNull();
    expect((await store.setTags("README.md", ["x"]))?.tags).toHaveLength(1);
    stop();
  });
});

describe("clean up", () => {
  it("counts the entries of files that are gone and removes them", async () => {
    const { api, store, stop } = await setup();
    await store.setTags("notes/ideas.md", ["old"]);
    api.probe.project.files.delete("notes/ideas.md");
    await later();
    expect(store.overview?.orphans).toBe(1);
    expect(await store.orphans()).toEqual(["notes/ideas.md"]);
    expect(await store.cleanUp()).toBe(1);
    await later();
    expect(store.overview?.orphans).toBe(0);
    stop();
  });
});

describe("the project goes", () => {
  it("forgets everything", async () => {
    const { store, stop } = await setup();
    store.openEditor("README.md");
    store.pick("alpha", false);
    stop();
    expect(store.index).toBeNull();
    expect(store.editing).toBeNull();
    expect(store.selected).toEqual([]);
  });
});
