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
const { editing } = await import("../editing/session.svelte");
const { reading } = await import("./reading.svelte");
const { WindowStore } = await import("../window.svelte");
const { syncPlugins } = await import("../plugins/instance.svelte");
const { installWindowKeys } = await import("../commands/install");
const { ui } = await import("../shell/ui.svelte");

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
  await mock.openPath(MOCK_FOLDER, "thisTab");
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
    "fmt-toml",
    "fmt-yaml",
    "fmt-dotenv",
    "fmt-ignore",
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
  ui.resetFocus();
  ui.closeFind();
  ui.closeFinder();
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

  it("a click on a web picture's chip fetches that picture; Ctrl+click opens it in the browser", async () => {
    const make = URL.createObjectURL;
    URL.createObjectURL = () => "blob:test/1";
    try {
      await show("README.md");
      await until(() => target.querySelector(".md-body p") !== null, "page");
      // the chip the page draws for `![logo](https://...)` (enhance)
      const drawn = document.createElement("span");
      drawn.className = "remote-image";
      drawn.setAttribute("data-url", "https://example.com/logo.png");
      drawn.setAttribute("aria-label", "logo");
      target.querySelector(".md-body p")?.append(drawn);
      expect(mock.probe.remote.asked).toEqual([]);
      const chip = (): HTMLElement | null => target.querySelector<HTMLElement>(".remote-image");
      const url = chip()?.getAttribute("data-url") ?? "";
      chip()?.dispatchEvent(
        new MouseEvent("click", { bubbles: true, cancelable: true, button: 0, ctrlKey: true }),
      );
      expect(mock.probe.web.at(-1)).toBe(url);
      expect(mock.probe.remote.asked).toEqual([]);
      chip()?.dispatchEvent(
        new MouseEvent("click", { bubbles: true, cancelable: true, button: 0 }),
      );
      await until(() => target.querySelector("img.remote-loaded") !== null, "picture shown");
      expect(mock.probe.remote.asked).toEqual([url]);
      expect(target.querySelector("img.remote-loaded")?.getAttribute("src")).toBe("blob:test/1");
    } finally {
      URL.createObjectURL = make;
    }
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

  it("the picture button chooses pictures in a file dialog and puts each next to the document", async () => {
    await import("../editing/visual/jsdomShims");
    await show("notes/ideas.md");
    await until(() => modeButton("Edit") !== undefined, "mode switch");
    modeButton("Edit")?.click();
    const button = (): HTMLButtonElement | null =>
      target.querySelector<HTMLButtonElement>("button[aria-label=Image]");
    await until(() => button()?.disabled === false, "picture button");
    button()?.click();
    await until(() => target.querySelector("form.popover") !== null, "picture popover");
    const choose = [...target.querySelectorAll<HTMLButtonElement>("form.popover button")].find(
      (b) => b.textContent?.trim() === "Choose a picture…",
    );
    expect(choose).toBeDefined();
    choose?.click();
    await until(() => editing.state.s === "dirty", "picture inserted");
    expect(target.querySelector("form.popover")).toBeNull();
    expect(editing.text).toContain("![](ideas-1.png)");
  });

  it("the diagram preview panel is off; its button puts it beside the editor, never over the text", async () => {
    await show("README.md");
    await until(() => modeButton("Source") !== undefined, "mode switch");
    modeButton("Source")?.click();
    await until(() => target.querySelector(".cm-editor") !== null, "source editor");
    expect(target.querySelector(".source .preview")).toBeNull();
    const toggle = target.querySelector<HTMLButtonElement>("button[aria-label='Diagram preview']");
    expect(toggle?.getAttribute("aria-pressed")).toBe("false");
    toggle?.click();
    await until(() => target.querySelector(".source .preview") !== null, "panel");
    // a sibling of the editor, not inside it
    const panel = target.querySelector(".source .preview");
    expect(panel?.closest(".cm-editor")).toBeNull();
    expect(panel?.previousElementSibling?.classList.contains("cm")).toBe(true);
    expect(panel?.textContent).toContain("mermaid or plantuml");
    toggle?.click();
    await until(() => target.querySelector(".source .preview") === null, "panel off");
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

describe("a narrow header", () => {
  // jsdom has no layout: the header's width is set by hand and the observer told
  const observers: (() => void)[] = [];
  const header = (): HTMLElement | null => target.querySelector<HTMLElement>(".doc-header");
  const labels = (): string[] =>
    [...(header()?.querySelectorAll<HTMLButtonElement>(":scope > button") ?? [])].map(
      (b) => b.getAttribute("aria-label") ?? "",
    );
  const modeButtons = (): HTMLButtonElement[] => [
    ...target.querySelectorAll<HTMLButtonElement>(".modes button"),
  ];
  function resize(px: number): void {
    const el = header();
    if (!el) throw new Error("no header");
    Object.defineProperty(el, "clientWidth", { value: px, configurable: true });
    for (const notify of observers) notify();
    flushSync();
  }

  beforeEach(() => {
    observers.length = 0;
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(notify: () => void) {
          observers.push(notify);
        }
        observe(): void {}
        disconnect(): void {}
      },
    );
  });

  it("folds secondary actions into the more menu, then turns the mode switch to icons; nothing is lost", async () => {
    await show("README.md");
    await until(() => modeButtons().length === 3, "mode switch");
    expect(labels()).toContain("Reveal in tree");
    resize(600);
    expect(labels()).not.toContain("Reveal in tree");
    expect(labels()).not.toContain("Find in page");
    expect(labels()).toContain("More actions");
    expect(modeButtons().map((b) => b.textContent?.trim())).toEqual(["View", "Edit", "Source"]);
    expect(target.querySelector(".saved")).toBeNull();
    // the folded actions lead the more menu and run from there
    header()?.querySelector<HTMLButtonElement>("button[aria-label='More actions']")?.click();
    flushSync();
    const items = [...target.querySelectorAll<HTMLButtonElement>("[role=menuitem]")];
    expect(items.map((b) => b.textContent?.trim()).slice(0, 4)).toEqual([
      "Reveal in tree",
      "Copy reference",
      "Open in external editor",
      "Find in page",
    ]);
    expect(target.querySelector("[role=menu] .heading")?.textContent).toMatch(/saved/);
    items[3]?.click();
    flushSync();
    expect(ui.finding).toBe(true);
    expect(target.querySelector("[role=menu]")).toBeNull();
    // very narrow: icons with their words as labels
    resize(200);
    expect(modeButtons().map((b) => b.textContent?.trim())).toEqual(["", "", ""]);
    expect(modeButtons().map((b) => b.getAttribute("aria-label"))).toEqual([
      "View",
      "Edit",
      "Source",
    ]);
    expect(labels()).toEqual(
      expect.arrayContaining(["Back", "Forward", "Reading width and zoom", "More actions"]),
    );
    // wide again: every button back in the row
    resize(1200);
    expect(labels()).toEqual(expect.arrayContaining(["Reveal in tree", "Find in page"]));
    expect(modeButtons()[0]?.textContent?.trim()).toBe("View");
  });

  it("Source adds Save and the diagram toggle: the same width folds sooner than in View", async () => {
    await show("README.md");
    await until(() => modeButtons().length === 3, "mode switch");
    resize(750);
    expect(labels()).toContain("Reveal in tree");
    await import("../editing/visual/jsdomShims");
    modeButtons()[2]?.click();
    await until(() => target.querySelector(".cm-editor") !== null, "source editor");
    flushSync();
    expect(labels()).not.toContain("Reveal in tree");
    expect(modeButtons()[2]?.textContent?.trim()).toBe("Source");
    resize(500);
    expect(modeButtons()[2]?.getAttribute("aria-label")).toBe("Source");
    const save = [...target.querySelectorAll<HTMLButtonElement>(".doc-header .btn")].find(
      (b) => b.getAttribute("aria-label") === "Save",
    );
    expect(save?.textContent?.trim()).toBe("");
  });
});

describe("config files", () => {
  const modeButton = (label: string): HTMLButtonElement | undefined =>
    [...target.querySelectorAll<HTMLButtonElement>(".modes button")].find(
      (b) => b.textContent?.trim() === label,
    );

  async function open(path: string): Promise<void> {
    mock.probe.project.files.set(path, "placeholder");
    await show(path);
    await until(() => target.querySelector(".sheet") !== null, `the sheet of ${path}`);
  }

  it("reads as a sheet, with Edit and Source one click away", async () => {
    await open("Cargo.toml");
    expect(target.querySelector(".crumb.current")?.textContent).toBe("Cargo.toml");
    expect(target.querySelectorAll(".sheet .card").length).toBeGreaterThan(2);
    expect(modeButton("Edit")?.disabled).toBe(false);
    expect(modeButton("Source")?.disabled).toBe(false);
    expect(target.querySelector(".sheet .hit")).toBeNull();
  });

  it("Edit makes the values take typing; the document is unsaved until it is saved", async () => {
    await open("Cargo.toml");
    modeButton("Edit")?.click();
    await until(() => target.querySelector(".sheet .hit") !== null, "editable values");
    expect(editing.mode).toBe("edit");
    const row = [...target.querySelectorAll<HTMLElement>(".sheet .row")].find(
      (r) => r.querySelector(".key")?.textContent === "version",
    );
    row?.querySelector<HTMLButtonElement>(".hit")?.click();
    flushSync();
    const input = row?.querySelector<HTMLInputElement>("input");
    if (!input) throw new Error("no input");
    input.value = "1.0.0";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await until(() => editing.state.s === "dirty", "unsaved");
    expect(editing.text).toContain('version = "1.0.0"');
    // back to View asks first: the edit is not saved
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "s", ctrlKey: true }));
    await until(() => editing.state.s === "clean", "saved");
    expect(mock.probe.edit.saves.at(-1)?.edits).toHaveLength(1);
  });

  it("Source shows the raw file: secret values as they are, nothing drawn over the text", async () => {
    await open(".env.example");
    // View masks them
    expect(target.textContent).not.toContain("sk-live-abc123");
    modeButton("Source")?.click();
    await until(() => target.querySelector(".cm-editor") !== null, "source editor");
    await until(
      () => target.querySelector(".cm-content")?.textContent?.includes("sk-live-abc123") === true,
      "raw values",
    );
    expect(target.querySelector(".cm-content")?.textContent).toContain("pa55w0rd");
    expect(target.querySelector(".cm-secret")).toBeNull();
    expect(target.querySelector(".cm-widgetBuffer, .cm-foldPlaceholder, .preview")).toBeNull();
  });
});

describe("focus mode", () => {
  const modeButton = (label: string): HTMLButtonElement | undefined =>
    [...target.querySelectorAll<HTMLButtonElement>(".modes button")].find(
      (b) => b.textContent?.trim() === label,
    );
  const chrome = (): HTMLElement | null => target.querySelector<HTMLElement>(".chrome");
  const escape = (init: KeyboardEventInit = {}, on: EventTarget = window): KeyboardEvent => {
    const event = new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
      ...init,
    });
    on.dispatchEvent(event);
    return event;
  };

  it("folds the header and the strips away but keeps them mounted; the page stays", async () => {
    await show("README.md");
    await until(() => target.querySelector(".crumb.current") !== null, "header");
    const header = target.querySelector(".crumb.current");
    expect(chrome()?.classList.contains("folded")).toBe(false);
    ui.setFocus("on");
    await tick();
    expect(chrome()?.classList.contains("folded")).toBe(true);
    // inert: no Tab stop, no clicks, no screen reader text; still the same nodes
    expect(chrome()?.inert).toBe(true);
    expect(target.querySelector(".crumb.current")).toBe(header);
    expect(target.querySelectorAll(".md-body > *").length).toBeGreaterThan(5);
    ui.setFocus("off");
    await tick();
    expect(chrome()?.classList.contains("folded")).toBe(false);
    expect(chrome()?.inert).toBe(false);
  });

  it("the transition class is there only for the moment after a toggle", async () => {
    vi.useFakeTimers();
    try {
      await show("README.md");
      await until(() => chrome() !== null, "header");
      ui.setFocus("on");
      await tick();
      expect(chrome()?.classList.contains("clip")).toBe(true);
      vi.advanceTimersByTime(300);
      await tick();
      expect(chrome()?.classList.contains("clip")).toBe(false);
      expect(chrome()?.classList.contains("folded")).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it("Escape leaves it; the same key toggles it", async () => {
    await show("README.md");
    await until(() => chrome() !== null, "header");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "f", ctrlKey: true, altKey: true }));
    expect(ui.focus).toBe("on");
    const event = escape();
    expect(ui.focus).toBe("off");
    expect(event.defaultPrevented).toBe(true);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "f", ctrlKey: true, altKey: true }));
    expect(ui.focus).toBe("on");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "f", ctrlKey: true, altKey: true }));
    expect(ui.focus).toBe("off");
  });

  it("Escape in View does nothing outside focus mode", async () => {
    await show("README.md");
    await until(() => chrome() !== null, "header");
    const event = escape();
    expect(ui.focus).toBe("off");
    expect(event.defaultPrevented).toBe(false);
  });

  it("Escape is left to what is innermost: a field, a dialog, an overlay, a pop-up, the zoom", async () => {
    await show("README.md");
    await until(() => chrome() !== null, "header");
    ui.setFocus("on");
    const input = document.createElement("input");
    document.body.append(input);
    escape({}, input);
    expect(ui.focus).toBe("on");
    input.remove();
    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    const inner = document.createElement("button");
    dialog.append(inner);
    document.body.append(dialog);
    escape({}, inner);
    expect(ui.focus).toBe("on");
    dialog.remove();
    const slash = document.createElement("div");
    slash.className = "slash";
    document.body.append(slash);
    escape();
    expect(ui.focus).toBe("on");
    slash.remove();
    ui.openFinder("quick");
    escape();
    expect(ui.focus).toBe("on");
    ui.closeFinder();
    documents.zoomed = { src: "x.png", alt: "" };
    escape();
    expect(ui.focus).toBe("on");
    documents.zoomed = null;
    // with the keys that are not a plain Escape, nor while composing
    escape({ ctrlKey: true });
    escape({ altKey: true });
    escape({ metaKey: true });
    escape({ isComposing: true });
    expect(ui.focus).toBe("on");
    escape();
    expect(ui.focus).toBe("off");
  });

  it("with the find bar open, the first Escape closes the bar and focus mode stays", async () => {
    await show("README.md");
    await until(() => chrome() !== null, "header");
    ui.setFocus("on");
    ui.openFind();
    await tick();
    expect(
      target.querySelector("input[type=search], .find input, [role=search] input") !== null ||
        ui.finding,
    ).toBe(true);
    escape();
    expect(ui.finding).toBe(false);
    expect(ui.focus).toBe("on");
    escape();
    expect(ui.focus).toBe("off");
  });

  it("the Edit toolbar goes away in focus mode and the first Escape leaves focus, not the editor", async () => {
    await import("../editing/visual/jsdomShims");
    await show("README.md");
    await until(() => modeButton("Edit") !== undefined, "mode switch");
    modeButton("Edit")?.click();
    await until(
      () => target.querySelector("[role=toolbar][aria-label=Formatting]") !== null,
      "toolbar",
    );
    ui.setFocus("on");
    await until(
      () => target.querySelector("[role=toolbar][aria-label=Formatting]") === null,
      "no toolbar",
    );
    expect(target.querySelector(".ProseMirror")).not.toBeNull();
    // the editors see Escape too: it is taken before they do
    const editor = target.querySelector(".ProseMirror") as HTMLElement;
    const event = escape({}, editor);
    expect(event.defaultPrevented).toBe(true);
    expect(ui.focus).toBe("off");
    await until(
      () => target.querySelector("[role=toolbar][aria-label=Formatting]") !== null,
      "toolbar back",
    );
    expect(target.querySelector(".ProseMirror")).not.toBeNull();
    expect(editing.mode).toBe("edit");
  });

  it("in Source the first Escape leaves focus; the second leaves the editor for View", async () => {
    await show("README.md");
    await until(() => modeButton("Source") !== undefined, "mode switch");
    modeButton("Source")?.click();
    await until(() => target.querySelector(".cm-editor") !== null, "source editor");
    ui.setFocus("on");
    escape();
    expect(ui.focus).toBe("off");
    expect(target.querySelector(".cm-editor")).not.toBeNull();
    escape();
    await until(() => target.querySelector(".cm-editor") === null, "back to View");
  });
});
