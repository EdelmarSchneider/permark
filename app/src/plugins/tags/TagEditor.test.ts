// @vitest-environment jsdom
// The tag editor and the tags view, mounted against the mock backend.
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER } from "../../api/mock";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});
Element.prototype.scrollIntoView = () => {};

const { api } = await import("../../api");
const { default: TagEditor } = await import("./TagEditor.svelte");
const { default: TagsPanel } = await import("./TagsPanel.svelte");
const { default: TagStrip } = await import("./TagStrip.svelte");
const { default: SearchTagFilter } = await import("./SearchTagFilter.svelte");
const { tags } = await import("./store.svelte");
const { windowStore } = await import("../../core/window.svelte");
const { documents } = await import("../../core/doc/document.svelte");
const { searchTags } = await import("../../core/search/tags.svelte");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;

let target: HTMLElement;
let component: Record<string, unknown> | null = null;
let stop: (() => void) | null = null;
let started = false;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
}

function type(input: HTMLInputElement, text: string): void {
  input.value = text;
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

function press(el: Element, key: string): void {
  el.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
}

const chips = (): string[] =>
  Array.from(target.querySelectorAll(".chips .chip .name")).map((n) => n.textContent ?? "");

async function open(): Promise<void> {
  await mock.openPath(MOCK_FOLDER, "thisTab");
  if (!started) {
    await windowStore.start();
    started = true;
  }
  stop = tags.start();
  await until(() => tags.index !== null && tags.overview !== null, "the tags are loaded");
}

beforeEach(() => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  stop?.();
  stop = null;
  searchTags.setProvider(null);
  tags.closeEditor();
  mock.probe.plugins.organizing.reset();
  for (const path of [...mock.probe.project.files.keys()]) {
    if (path === "fm.md") mock.probe.project.files.delete(path);
  }
  vi.restoreAllMocks();
});

describe("the tag editor", () => {
  async function editor(path: string): Promise<HTMLInputElement> {
    await open();
    component = mount(TagEditor, { target, props: { path } });
    flushSync();
    await until(
      () => target.querySelector<HTMLInputElement>("input.field")?.disabled === false,
      "ready",
    );
    const field = target.querySelector<HTMLInputElement>("input.field");
    if (!field) throw new Error("no field");
    return field;
  }

  it("adds a tag with Enter, one with a comma, and shows both saved", async () => {
    const field = await editor("docs/guide.md");
    type(field, "Needs Review");
    press(field, "Enter");
    await until(() => chips().includes("needs-review"), "the first tag");
    type(field, "alpha,");
    await until(() => chips().includes("alpha"), "the second tag");
    expect(mock.probe.plugins.organizing.appTags("docs/guide.md")).toEqual([
      "needs-review",
      "alpha",
    ]);
    expect(field.value).toBe("");
  });

  it("removes the last tag with Backspace in an empty field, and any tag with its cross", async () => {
    const field = await editor("docs/guide.md");
    type(field, "one,");
    await until(() => chips().includes("one"), "one");
    type(field, "two,");
    await until(() => chips().includes("two"), "two");
    press(field, "Backspace");
    await until(() => !chips().includes("two"), "two is gone");
    target.querySelector<HTMLElement>(".remove")?.click();
    await until(() => chips().length === 0, "all gone");
  });

  it("locks the tags of the front matter and still adds app tags next to them", async () => {
    mock.probe.project.add("fm.md", "---\ntags: [alpha, beta]\n---\n# T\n");
    const field = await editor("fm.md");
    expect(chips()).toEqual(["alpha", "beta"]);
    expect(target.querySelectorAll(".chips .remove")).toHaveLength(0);
    expect(target.querySelectorAll(".chips .lock")).toHaveLength(2);
    // a tag the file already has is not added twice
    type(field, "alpha");
    press(field, "Enter");
    type(field, "gamma");
    press(field, "Enter");
    await until(() => chips().includes("gamma"), "gamma");
    expect(chips().filter((c) => c === "alpha")).toHaveLength(1);
    expect(mock.probe.plugins.organizing.appTags("fm.md")).toEqual(["gamma"]);
  });

  it("suggests the tags of the project and adds one with a click", async () => {
    mock.probe.project.add("fm.md", "---\ntags: [alpha, beta]\n---\n# T\n");
    await mock.pluginCall("tags.setTags", { path: "README.md", tags: ["roadmap"] });
    const field = await editor("docs/guide.md");
    await until(() => (tags.overview?.tags.length ?? 0) >= 3, "the overview");
    type(field, "ro");
    await until(() => target.querySelector(".suggestions") !== null, "suggestions");
    expect(target.querySelector(".suggestions")?.textContent).toContain("roadmap");
    target.querySelector<HTMLElement>(".suggestions button")?.click();
    await until(() => chips().includes("roadmap"), "added");
  });

  it("keeps the description as the person types it, on one line, with its counter", async () => {
    await editor("docs/guide.md");
    const box = target.querySelector<HTMLInputElement>("#tags-description");
    if (!box) throw new Error("no description");
    type(box, "How the guide reads");
    flushSync();
    expect(target.querySelector(".counter")?.textContent).toBe("19/160");
    box.dispatchEvent(new FocusEvent("blur"));
    await until(() => tags.descriptionOf("docs/guide.md") === "How the guide reads", "saved");
    expect(tags.descriptionOf("docs/guide.md")).toBe("How the guide reads");
  });

  it("closes with Escape and says where the tags are kept", async () => {
    await editor("docs/guide.md");
    expect(target.textContent).toContain("Saved in .permark/meta.toml");
    tags.openEditor("docs/guide.md");
    const dialog = target.querySelector<HTMLElement>('[role="dialog"]');
    if (!dialog) throw new Error("no dialog");
    press(dialog, "Escape");
    await until(() => tags.editing === null, "closed");
  });

  it("is read-only while the file cannot be read, and says why", async () => {
    mock.probe.plugins.organizing.tagsProblem({ message: "bad token", line: 7 });
    const field = await editor("docs/guide.md").catch(() => null);
    void field;
    await until(() => target.textContent?.includes("read-only") === true, "the notice");
    expect(target.textContent).toContain("line 7");
    expect(target.querySelector<HTMLInputElement>("input.field")?.disabled).toBe(true);
    mock.probe.plugins.organizing.tagsProblem(null);
  });
});

describe("the strip under the header", () => {
  it("shows the tags and the description of the document, and opens the editor", async () => {
    await open();
    await mock.pluginCall("tags.setTags", { path: "docs/guide.md", tags: ["spec"] });
    await mock.pluginCall("tags.setDescription", {
      path: "docs/guide.md",
      description: "About it",
    });
    vi.spyOn(documents, "shown", "get").mockReturnValue("docs/guide.md");
    component = mount(TagStrip, { target });
    flushSync();
    await until(() => target.querySelector(".strip") !== null, "the strip");
    expect(target.querySelector(".chip .name")?.textContent).toBe("spec");
    expect(target.querySelector(".description")?.textContent).toBe("About it");
    target.querySelector<HTMLElement>(".edit")?.click();
    expect(tags.editing?.path).toBe("docs/guide.md");
    tags.closeEditor();
  });

  it("draws nothing for a document that has neither", async () => {
    await open();
    vi.spyOn(documents, "shown", "get").mockReturnValue("README.md");
    component = mount(TagStrip, { target });
    flushSync();
    await tick();
    expect(target.querySelector(".strip")).toBeNull();
  });
});

describe("the tags tab", () => {
  it("lists the tags with counts and the files of the selection", async () => {
    mock.probe.project.add("fm.md", "---\ntags: [alpha, beta]\n---\n# T\n");
    await open();
    component = mount(TagsPanel, { target });
    flushSync();
    await until(() => target.querySelectorAll(".list .chip").length === 2, "the tags");
    expect(target.querySelector(".list")?.textContent).toContain("alpha");
    target.querySelector<HTMLElement>(".list .chip .face")?.click();
    await until(() => target.querySelector(".files") !== null, "the files");
    expect(target.querySelector(".files")?.textContent).toContain("fm.md");
    // the filter box
    const find = target.querySelector<HTMLInputElement>('input[type="search"]');
    if (!find) throw new Error("no filter box");
    type(find, "bet");
    flushSync();
    expect(target.querySelectorAll(".list .chip")).toHaveLength(1);
    expect(target.querySelector(".list")?.textContent).toContain("beta");
  });

  it("offers the untagged documents and shows an unreadable file", async () => {
    await open();
    component = mount(TagsPanel, { target });
    flushSync();
    await until(() => target.querySelector(".untagged") !== null, "untagged row");
    target.querySelector<HTMLElement>(".untagged")?.click();
    await until(() => target.querySelector(".files") !== null, "the files");
    expect(target.querySelector(".files")?.textContent).toContain("README.md");
    mock.probe.plugins.organizing.tagsProblem({ message: "oops", line: 2 });
    await until(() => target.querySelector(".warn") !== null, "the notice");
    expect(target.querySelector(".warn")?.textContent).toContain("oops");
    mock.probe.plugins.organizing.tagsProblem(null);
  });

  it("asks before it cleans up the entries of missing files", async () => {
    await open();
    await mock.pluginCall("tags.setTags", { path: "notes/ideas.md", tags: ["old"] });
    mock.probe.project.files.delete("notes/ideas.md");
    component = mount(TagsPanel, { target });
    flushSync();
    await until(() => target.querySelector("footer") !== null, "the footer");
    expect(target.querySelector("footer")?.textContent).toContain("Clean up (1)");
    target.querySelector<HTMLElement>("footer .link")?.click();
    flushSync();
    expect(target.querySelector("footer")?.textContent).toContain("1 missing-file entry");
    target.querySelector<HTMLElement>("footer .btn--primary")?.click();
    await until(() => target.querySelector("footer") === null, "cleaned");
  });
});

describe("the tag filter of the search boxes", () => {
  it("shows the selected tags as chips and adds one from the list", async () => {
    mock.probe.project.add("fm.md", "---\ntags: [alpha, beta]\n---\n# T\n");
    await open();
    searchTags.setProvider({ list: () => tags.overview?.tags ?? [] });
    component = mount(SearchTagFilter, { target });
    flushSync();
    await until(() => target.querySelector(".plus") !== null, "the add button");
    target.querySelector<HTMLElement>(".plus")?.click();
    flushSync();
    expect(target.querySelectorAll(".menu button")).toHaveLength(2);
    target.querySelector<HTMLElement>(".menu button")?.click();
    flushSync();
    expect(searchTags.selected).toEqual(["alpha"]);
    expect(target.querySelector(".chip .name")?.textContent).toBe("alpha");
    expect(target.querySelectorAll(".menu")).toHaveLength(0);
    target.querySelector<HTMLElement>(".chip .remove")?.click();
    flushSync();
    expect(searchTags.selected).toEqual([]);
  });
});
