// @vitest-environment jsdom
// The ways into the list of earlier versions: a command, an entry of the Explorer menu of a file, an entry of the
// document's menu. A list is of the document shown: asked for another file, the window goes to that file first.
import "../../core/testing/media";
import { flushSync } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER } from "../../api/mock";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { documents } = await import("../../core/doc/document.svelte");
const { setLeaveGuard } = await import("../../core/editing/guard");
const { ContributionRegistry, PluginHost, pluginLoaders } =
  await import("../../core/plugins/registry.svelte");
const { review } = await import("./store.svelte");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;

const GUIDE = "docs/guide.md";

let registry: InstanceType<typeof ContributionRegistry>;
let host: InstanceType<typeof PluginHost>;
let stopFollowing: (() => void) | null = null;
/** The document the window showed each time the list was asked to open. */
let opened: (string | null)[] = [];

/** Lets the backend's answers and the effects that follow them run. */
async function settle(): Promise<void> {
  for (let i = 0; i < 6; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
}

/** The window goes to `path` (as a click in the Explorer does). */
async function show(path: string): Promise<void> {
  await mock.showDocument(path);
  await vi.waitFor(() => expect(documents.view?.path).toBe(path), { timeout: 10_000 });
  flushSync();
}

const command = () => registry.of("command").find((c) => c.id === "review.versions");
const treeEntry = () => registry.of("treeMenu").find((e) => e.owner === "review");
const docEntry = () => registry.of("docMenu").find((e) => e.owner === "review");

beforeEach(async () => {
  await mock.openPath(MOCK_FOLDER, "thisTab");
  const boot = await mock.windowBoot();
  if (!boot.ok || boot.value.view.state !== "showing") throw new Error("no project");
  documents.setWorkspace(boot.value.view.workspace);
  // the window shows what the backend says
  stopFollowing = await mock.on("window-changed", (event) => {
    if (event.view.state === "showing") void documents.show(event.view.document);
  });
  await mock.showDocument(null);
  await settle();
  registry = new ContributionRegistry();
  host = new PluginHost(registry, pluginLoaders());
  await host.enable("review");
  opened = [];
  vi.spyOn(review, "openVersions").mockImplementation(async () => {
    opened.push(documents.path);
  });
});

afterEach(async () => {
  host.disable("review");
  stopFollowing?.();
  documents.setWorkspace(null);
  vi.restoreAllMocks();
  await settle();
});

describe("the command", () => {
  it("is offered only with a document open, has no key of its own, and opens the list", async () => {
    expect(command()?.label).toBe("review.versions.command");
    expect(command()?.when?.()).toBe(false);
    expect(registry.of("shortcut").some((s) => s.command === "review.versions")).toBe(false);
    await show("README.md");
    expect(command()?.when?.()).toBe(true);
    await command()?.run();
    expect(opened).toEqual(["README.md"]);
  });
});

describe("the entry of the document's menu", () => {
  it("opens the list of the document shown", async () => {
    await show("README.md");
    expect(docEntry()?.label).toBe("review.versions.menu");
    expect(docEntry()?.applies?.("README.md") ?? true).toBe(true);
    docEntry()?.run("README.md");
    expect(opened).toEqual(["README.md"]);
  });
});

describe("the entry of the Explorer menu", () => {
  it("is there for a file, not for a folder", () => {
    expect(treeEntry()?.label).toBe("review.versions.menu");
    expect(treeEntry()?.applies(GUIDE, false)).toBe(true);
    expect(treeEntry()?.applies("docs", true)).toBe(false);
  });

  it("on the document shown: its list opens at once", async () => {
    await show(GUIDE);
    const go = vi.spyOn(documents, "navigate");
    treeEntry()?.run([GUIDE]);
    await settle();
    expect(opened).toEqual([GUIDE]);
    expect(go).not.toHaveBeenCalled();
  });

  it("on another file: the window goes to that file first, then its list opens, once", async () => {
    await show("README.md");
    treeEntry()?.run([GUIDE]);
    expect(opened).toEqual([]);
    await vi.waitFor(() => expect(opened).toHaveLength(1), { timeout: 10_000 });
    // the list was asked for when the window was on the file already
    expect(opened).toEqual([GUIDE]);
    // coming back to the file later is no ask for its list
    await show("README.md");
    await show(GUIDE);
    await settle();
    expect(opened).toEqual([GUIDE]);
  });

  it("with no document shown: the file opens, then its list", async () => {
    expect(documents.path).toBeNull();
    treeEntry()?.run([GUIDE]);
    await vi.waitFor(() => expect(opened).toEqual([GUIDE]), { timeout: 10_000 });
  });

  it("the person keeps editing the document open: no list, not at a later visit of the file either", async () => {
    await show("README.md");
    // the editor holds text that is not saved, and the person answers "keep editing"
    const off = setLeaveGuard(() => Promise.resolve(false));
    treeEntry()?.run([GUIDE]);
    await settle();
    expect(documents.path).toBe("README.md");
    off();
    await show(GUIDE);
    await settle();
    expect(opened).toEqual([]);
  });

  it("the window went to another file meanwhile: the list asked for is not wanted any more", async () => {
    await show("README.md");
    // the move to the file never comes (the backend refused it)
    vi.spyOn(documents, "navigate").mockImplementation(() => undefined);
    treeEntry()?.run([GUIDE]);
    await settle();
    await show("src/main.rs");
    await show(GUIDE);
    await settle();
    expect(opened).toEqual([]);
  });

  it("switched off while a file is on its way: nothing opens, nothing is left", async () => {
    await show("README.md");
    vi.spyOn(documents, "navigate").mockImplementation(() => undefined);
    treeEntry()?.run([GUIDE]);
    await settle();
    expect(host.disable("review")).toBe(0);
    expect(registry.items).toHaveLength(0);
    await show(GUIDE);
    await settle();
    expect(opened).toEqual([]);
  });
});
