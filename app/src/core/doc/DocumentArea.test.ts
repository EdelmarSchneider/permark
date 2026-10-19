// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WorkspaceView } from "../../api";
import { MOCK_FOLDER } from "../../api/mock";

// the components talk to the mock backend (one per test file)
vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

// jsdom has no media queries; the look asks for the system's dark mode
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});

const { api } = await import("../../api");
const { default: DocumentArea } = await import("./DocumentArea.svelte");
const { documents } = await import("./document.svelte");
const { reading } = await import("./reading.svelte");
const { WindowStore } = await import("../window.svelte");
const { syncPlugins } = await import("../plugins/instance.svelte");
const { installWindowKeys } = await import("../commands/install");

// `api` is the mock created by the factory above
const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;

type Shown = Extract<WorkspaceView, { kind: "project" }>;

let component: Record<string, unknown> | null = null;
let stopKeys: (() => void) | null = null;
let target: HTMLElement;
let workspace: Shown;
let store: InstanceType<typeof WindowStore>;
let stopFollowing: (() => void) | null = null;

// jsdom has no layout: a range has no box
Range.prototype.getBoundingClientRect = () => new DOMRect();

async function show(path: string): Promise<void> {
  await mock.showDocument(path);
  await tick();
}

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 3000, interval: 10 });
}

beforeEach(async () => {
  vi.stubGlobal("navigator", { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } });
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
  await mock.openPath(MOCK_FOLDER, "thisWindow");
  const boot = await mock.windowBoot();
  if (
    !boot.ok ||
    boot.value.view.state !== "showing" ||
    boot.value.view.workspace.kind !== "project"
  ) {
    throw new Error("no project");
  }
  workspace = boot.value.view.workspace;
  store = new WindowStore(mock);
  await store.start();
  reading.setWidth("comfortable");
  reading.setZoom(100);
  // the plugins that have a view and do not need a big library in this test
  syncPlugins([
    "highlight",
    "frontmatter-card",
    "math",
    "text-view",
    "json-view",
    "media",
    "xml-view",
  ]);
  // the window shows what the backend says; the area follows
  stopFollowing = await mock.on("window-changed", (event) => {
    if (event.view.state === "showing") void documents.show(event.view.document);
  });
  // the keys are commands now: the window's listener runs them, as in the app
  stopKeys = installWindowKeys();
  component = mount(DocumentArea, { target, props: { workspace, document: null, store } });
  flushSync();
});

afterEach(async () => {
  stopKeys?.();
  stopFollowing?.();
  if (component) await unmount(component);
  component = null;
  documents.setWorkspace(null);
  syncPlugins([]);
  vi.unstubAllGlobals();
});

describe("the page", () => {
  it("shows the header, the properties card, the blocks and the outline data", async () => {
    await show("README.md");
    await until(() => target.querySelectorAll(".md-body > *").length > 5, "blocks");
    expect(target.querySelector(".crumb.current")?.textContent).toBe("README.md");
    await until(() => target.querySelector(".card .grid") !== null, "front matter card");
    expect(target.querySelector(".card")?.textContent).toContain("Ana Souza");
    expect(target.querySelectorAll(".card .chip")).toHaveLength(3);
    // headings carry their anchor link
    expect(target.querySelector("h2#text-and-lists a.anchor")?.getAttribute("href")).toBe(
      "#text-and-lists",
    );
    // every block is marked with its id
    const marks = [...target.querySelectorAll(".md-body > [data-b]")].map((el) =>
      el.getAttribute("data-b"),
    );
    expect(new Set(marks).size).toBe(marks.length);
    expect(
      documents.view?.body.kind === "markdown" && documents.view.body.outline.length,
    ).toBeGreaterThan(5);
  });

  it("highlights code when it comes near, with the language label and a copy button", async () => {
    await show("README.md");
    await until(
      () =>
        [...target.querySelectorAll(".code-block .lang")].some((el) => el.textContent === "Rust"),
      "rust label",
    );
    const block = [...target.querySelectorAll(".code-block")].find(
      (el) => el.querySelector(".lang")?.textContent === "Rust",
    );
    expect(block?.querySelector(".hljs-keyword")).not.toBeNull();
    expect(block?.querySelector("button[aria-label]")).not.toBeNull();
    block?.querySelector<HTMLButtonElement>("button[aria-label]")?.click();
    await until(() => vi.mocked(navigator.clipboard.writeText).mock.calls.length > 0, "copied");
    expect(vi.mocked(navigator.clipboard.writeText).mock.calls[0]?.[0]).toContain(
      'println!("Hello, {name}!")',
    );
  });

  it("draws formulas and keeps pictures from the web out", async () => {
    await show("README.md");
    await until(() => target.querySelector(".katex") !== null, "katex");
    expect(target.querySelectorAll("[data-math-style]").length).toBeGreaterThan(1);
    // a picture on the web is a chip, not a request
    expect(target.querySelector("img[src^='http']")).toBeNull();
  });

  it("marks links to missing documents and follows the ones that exist", async () => {
    await show("README.md");
    await until(() => target.querySelector("a.is-missing") !== null, "missing link marked");
    const missing = target.querySelector<HTMLAnchorElement>("a.is-missing");
    expect(missing?.getAttribute("href")).toBe("docs/nope.md");
    expect(missing?.title).toContain("docs/nope.md");
    const guide = target.querySelector<HTMLAnchorElement>('a[href="docs/guide.md"]');
    expect(guide?.classList.contains("is-missing")).toBe(false);
    // a click goes through the controller (history entry), not the webview
    const event = new MouseEvent("click", { bubbles: true, cancelable: true, button: 0 });
    guide?.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    await until(() => documents.path === "docs/guide.md", "navigated");
    expect(documents.trail.canBack).toBe(true);
  });

  it("opens web links in the browser and code references in the editor", async () => {
    await show("README.md");
    await until(() => target.querySelector(".code-ref") !== null, "code reference");
    const web = target.querySelector<HTMLAnchorElement>('a[href="https://example.com"]');
    web?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, button: 0 }));
    await until(() => mock.probe.web.includes("https://example.com"), "web link opened");
    const ref = target.querySelector<HTMLElement>(".code-ref");
    ref?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, button: 0 }));
    await until(() => mock.probe.editor.length > 0, "editor opened");
    expect(mock.probe.editor[0]).toMatchObject({ path: "src/main.rs" });
  });

  it("folds prompt blocks and marks variables", async () => {
    await show("CLAUDE.md");
    await until(() => target.querySelector("details.prompt-block") !== null, "prompt block");
    expect(target.querySelector("details.prompt-block summary")?.textContent).toBe("instructions");
    expect(target.querySelector(".tvar")?.textContent).toBe("{{user}}");
    expect(target.querySelector("a.at-ref")?.getAttribute("href")).toBe("/docs/guide.md");
    // the role badge
    await until(() => target.querySelector(".role") !== null, "role badge");
  });
});

describe("leaving", () => {
  it("lets go of the document and its outline when the area goes away", async () => {
    await show("README.md");
    await until(() => target.querySelectorAll(".md-body > *").length > 5, "blocks");
    expect(documents.view).not.toBeNull();
    // the window goes back to the welcome screen: the area is destroyed, nothing else tells the controller
    if (component) await unmount(component);
    component = null;
    expect(documents.view).toBeNull();
    expect(documents.workspaceId).toBeNull();
    expect(documents.currentHeading).toBe(-1);
  });
});

describe("reading", () => {
  it("zoom keys change the size and are remembered", async () => {
    await show("docs/guide.md");
    await until(() => target.querySelector(".md") !== null, "page");
    const page = (): string =>
      target.querySelector<HTMLElement>(".md")?.style.getPropertyValue("--zoom") ?? "";
    expect(page()).toBe("1");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "=", ctrlKey: true, bubbles: true }));
    await tick();
    expect(page()).toBe("1.1");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "0", ctrlKey: true, bubbles: true }));
    await tick();
    expect(page()).toBe("1");
    // not while typing in an IME
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "=", ctrlKey: true, isComposing: true, bubbles: true }),
    );
    await tick();
    expect(page()).toBe("1");
  });

  it("the reading width changes the column", async () => {
    await show("docs/guide.md");
    await until(() => target.querySelector(".md") !== null, "page");
    reading.setWidth("wide");
    await tick();
    expect(target.querySelector(".md")?.classList.contains("is-wide")).toBe(true);
    reading.setWidth("full");
    await tick();
    expect(target.querySelector(".md")?.classList.contains("is-full")).toBe(true);
  });

  it("finds in the page: count, next, and a clear when it closes", async () => {
    await show("docs/guide.md");
    await until(
      () => target.querySelector(".md-body")?.textContent?.includes("Alt+Left") === true,
      "text",
    );
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "f", ctrlKey: true, bubbles: true }));
    await tick();
    const input = target.querySelector<HTMLInputElement>(".find input");
    expect(input).not.toBeNull();
    if (!input) return;
    input.value = "back";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    expect(target.querySelector(".find .count")?.textContent).toMatch(/1 of \d+/);
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await tick();
    expect(target.querySelector(".find .count")?.textContent).toMatch(/2 of \d+/);
    input.value = "zzzzzz";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
    expect(target.querySelector(".find .count")?.textContent).toBe("No results");
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick();
    expect(target.querySelector(".find")).toBeNull();
  });

  it("Alt+Left goes back through the trail", async () => {
    await show("README.md");
    await documents.activateLink("docs/guide.md");
    await until(() => documents.path === "docs/guide.md", "guide");
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowLeft", altKey: true, bubbles: true }),
    );
    await until(() => documents.path === "README.md", "back");
  });
});

describe("other kinds of file", () => {
  it("shows text with line numbers and a language label", async () => {
    await show("src/main.rs");
    await until(() => target.querySelector(".text .gutter") !== null, "text view");
    expect(target.querySelector(".gutter")?.textContent).toBe("1\n2\n3");
    await until(() => target.querySelector(".text .lang")?.textContent === "Rust", "highlight");
  });

  it("shows JSON as a tree you can open, and the text on demand", async () => {
    await show("package.json");
    await until(() => target.querySelector(".structured [role=tree]") !== null, "tree");
    const rows = [...target.querySelectorAll(".structured .label")].map((el) => el.textContent);
    expect(rows).toEqual(expect.arrayContaining(["name", "version", "scripts"]));
    const toggle = [...target.querySelectorAll<HTMLButtonElement>(".structured .head")].find((b) =>
      b.textContent?.includes("scripts"),
    );
    toggle?.click();
    await tick();
    expect(target.querySelector(".structured")?.textContent).toContain("build");
    [...target.querySelectorAll<HTMLButtonElement>(".structured .segment")]
      .find((b) => b.textContent === "Source")
      ?.click();
    await tick();
    expect(target.querySelector(".structured .source")?.textContent).toContain('"keywords"');
  });

  it("shows a picture in place with its size controls", async () => {
    await show("docs/img/logo.png");
    await until(() => target.querySelector(".media img") !== null, "image");
    expect(target.querySelector(".media img")?.getAttribute("src")).toContain("data:image/svg+xml");
    expect(target.querySelector(".media .bar")).not.toBeNull();
  });

  it("says why a file has no page", async () => {
    await show("notes/todo.txt");
    await until(() => target.querySelector(".text") !== null, "text");
    mock.probe.project.add("blob.bin", "x");
    await show("blob.bin");
    await until(() => target.querySelector(".notice") !== null, "notice");
    expect(target.querySelector(".notice .title")?.textContent).toContain("can't be shown");
    expect(target.querySelector(".notice button")).not.toBeNull();
  });

  it("tells a document that cannot be opened", async () => {
    await show("gone.md");
    await until(() => target.querySelector("[role=alert]") !== null, "alert");
    expect(target.querySelector("[role=alert]")?.textContent).toContain("gone.md");
  });
});

describe("a document that changes under the reader", () => {
  it("keeps the nodes of unchanged blocks and shows a deleted document with a banner", async () => {
    await show("README.md");
    await until(() => target.querySelectorAll(".md-body > *").length > 5, "blocks");
    const first = target.querySelector(".md-body > *");
    mock.emit("document-changed", {
      window: "main",
      path: "README.md",
      change: "changed",
      version: "2",
    });
    await until(() => mock.probe.shown.known.length > 5, "asked with what it holds");
    await tick();
    // the node of an unchanged block is the same node
    expect(target.querySelector(".md-body > *")).toBe(first);
    mock.emit("document-changed", {
      window: "main",
      path: "README.md",
      change: "deleted",
      movedTo: "docs/README.md",
    });
    await until(() => target.querySelector(".banner") !== null, "banner");
    expect(target.querySelector(".banner")?.textContent).toContain("docs/README.md");
    expect(target.querySelector(".md.is-tombstone")).not.toBeNull();
    expect(target.querySelectorAll(".md-body > *").length).toBeGreaterThan(5);
  });
});

describe("editing", () => {
  const modeButton = (label: string): HTMLButtonElement | undefined =>
    [...target.querySelectorAll<HTMLButtonElement>(".modes button")].find(
      (b) => b.textContent?.trim() === label,
    );

  it("Edit shows the visual editor with its toolbar and the blocks it keeps; Ctrl+E goes back", async () => {
    await import("../editing/visual/jsdomShims");
    await show("README.md");
    await until(() => modeButton("Edit") !== undefined, "mode switch");
    modeButton("Edit")?.click();
    await until(() => target.querySelector(".ProseMirror") !== null, "visual editor");
    const errors = vi.spyOn(console, "error");
    await until(
      () => target.querySelector(".pm-locked") !== null || errors.mock.calls.length > 0,
      "visual document",
    );
    expect(errors.mock.calls[0]?.[0]).toBeUndefined();
    expect(target.querySelector("[role=toolbar][aria-label=Formatting]")).not.toBeNull();
    // the front matter stays as written, with the way to Source
    const locked = [...target.querySelectorAll<HTMLElement>(".pm-locked")];
    expect(locked.map((l) => `${l.dataset.kind}:${l.dataset.reason}`).join(",")).toContain(
      "frontMatter:frontMatter",
    );
    expect(target.querySelector(".reading")?.hasAttribute("hidden")).toBe(true);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "e", ctrlKey: true }));
    await until(() => target.querySelector(".ProseMirror") === null, "back to View");
    expect(target.querySelector(".reading")?.hasAttribute("hidden")).toBe(false);
  });

  it("Source shows the text editor; a text file has Source only", async () => {
    await show("README.md");
    await until(() => modeButton("Source") !== undefined, "mode switch");
    modeButton("Source")?.click();
    await until(() => target.querySelector(".cm-editor") !== null, "source editor");
    expect(target.querySelector(".cm-content")?.textContent).toContain("Reading showcase");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await until(() => target.querySelector(".cm-editor") === null, "back to View");
    await show("notes/todo.txt");
    await until(() => modeButton("Edit")?.disabled === true, "no visual editor for text");
    expect(modeButton("Source")?.disabled).toBe(false);
  });
});
