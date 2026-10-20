// @vitest-environment jsdom
// What plugins can do on the page: a strip under the header, extra blocks, a listener on the page root, a badge in the
// header, and being told which blocks a live update added or removed.
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Block } from "../../api";
import { MOCK_FOLDER } from "../../api/mock";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});

const { api } = await import("../../api");
const { default: DocumentArea } = await import("./DocumentArea.svelte");
const { documents } = await import("./document.svelte");
const { WindowStore } = await import("../window.svelte");
const { contributions } = await import("../plugins/instance.svelte");
const { NO_REQUIREMENT } = await import("../conditions/available");
const { Box } = await import("../testing/box.svelte");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;

let component: Record<string, unknown> | null = null;
let target: HTMLElement;
let stopFollowing: (() => void) | null = null;
const removals: (() => void)[] = [];

Range.prototype.getBoundingClientRect = () => new DOMRect();

async function show(path: string): Promise<void> {
  await mock.showDocument(path);
  await tick();
}

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 3000, interval: 10 });
}

function contribute(item: Parameters<typeof contributions.register>[0]): void {
  removals.push(contributions.register(item));
}

beforeEach(async () => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
  await mock.openPath(MOCK_FOLDER, "thisWindow");
  const boot = await mock.windowBoot();
  if (!boot.ok || boot.value.view.state !== "showing") throw new Error("no project");
  const workspace = boot.value.view.workspace;
  if (workspace.kind !== "project") throw new Error("no project");
  const store = new WindowStore(mock);
  await store.start();
  stopFollowing = await mock.on("window-changed", (event) => {
    if (event.view.state === "showing") void documents.show(event.view.document);
  });
  component = mount(DocumentArea, { target, props: { workspace, document: null, store } });
  flushSync();
});

afterEach(async () => {
  stopFollowing?.();
  if (component) await unmount(component);
  component = null;
  for (const remove of removals.splice(0)) remove();
  documents.setWorkspace(null);
});

const owner = "test";

describe("a strip under the header", () => {
  it("shows what plugins put there, in order, and drops it with the plugin", async () => {
    contribute({
      kind: "docBar",
      id: "b",
      order: 2,
      owner,
      requires: NO_REQUIREMENT,
      view: () => import("../testing/StripSecond.svelte"),
    });
    contribute({
      kind: "docBar",
      id: "a",
      order: 1,
      owner,
      requires: NO_REQUIREMENT,
      view: () => import("../testing/StripFirst.svelte"),
    });
    await show("README.md");
    await until(() => target.querySelectorAll(".strip").length === 2, "the strips");
    expect([...target.querySelectorAll(".strip")].map((s) => s.textContent)).toEqual([
      "first",
      "second",
    ]);
    // under the header, above the page
    const header = target.querySelector(".doc-header");
    const strip = target.querySelector(".strip");
    expect(strip && header?.compareDocumentPosition(strip)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });
});

describe("a badge in the header", () => {
  it("says what plugins say about the document", async () => {
    contribute({
      kind: "treeBadge",
      id: "git.badge",
      owner,
      requires: NO_REQUIREMENT,
      badge: (path, isDir) =>
        !isDir && path === "README.md" ? { text: "M", tone: "modified", title: "changed" } : null,
    });
    await show("README.md");
    await until(() => target.querySelector(".doc-header .badge") !== null, "the badge");
    expect(target.querySelector(".doc-header .badge")?.textContent).toBe("M");
    await show("docs/guide.md");
    await until(() => target.querySelector(".doc-header .badge") === null, "no badge");
  });
});

describe("blocks a plugin adds", () => {
  const extra: Block = {
    id: "rv:test",
    kind: "div",
    lineStart: 0,
    lineEnd: 0,
    html: '<div class="extra">added by a plugin</div>',
  };

  it("are on the page while the plugin wants them, and the page is made again when it says so", async () => {
    let on = false;
    contribute({
      kind: "docBlocks",
      id: "extra",
      owner,
      requires: NO_REQUIREMENT,
      transform: (blocks) => (on ? [extra, ...blocks] : blocks),
    });
    const updates: { added: readonly string[]; removed: readonly { id: string }[] }[] = [];
    const off = documents.onUpdated((u) => updates.push(u));
    await show("README.md");
    await until(() => target.querySelectorAll(".md-body > *").length > 5, "blocks");
    expect(target.querySelector(".extra")).toBeNull();
    const before = target.querySelectorAll(".md-body > *").length;
    const scroller = target.querySelector<HTMLElement>("[data-scroller]");
    if (scroller) scroller.scrollTop = 40;

    on = true;
    documents.refreshBlocks();
    await until(() => target.querySelector(".extra") !== null, "the added block");
    expect(target.querySelectorAll(".md-body > *").length).toBe(before + 1);
    expect(target.querySelector(".md-body > :first-child")?.className).toBe("extra");
    expect(updates.at(-1)).toEqual({ added: ["rv:test"], removed: [] });
    // the page was updated, not built again: the place stays
    expect(scroller?.scrollTop).toBe(40);

    on = false;
    documents.refreshBlocks();
    await until(() => target.querySelector(".extra") === null, "the block gone");
    expect(updates.at(-1)).toEqual({ added: [], removed: [{ id: "rv:test", after: null }] });
    off();
  });

  it("a fresh document is not an update: nobody is told of its blocks", async () => {
    const updates: unknown[] = [];
    const off = documents.onUpdated((u) => updates.push(u));
    await show("README.md");
    await until(() => target.querySelectorAll(".md-body > *").length > 5, "blocks");
    await show("docs/guide.md");
    await until(() => target.querySelector(".crumb.current")?.textContent === "guide.md", "guide");
    expect(updates).toEqual([]);
    off();
  });
});

describe("a listener on the page root", () => {
  it("is set while the page exists and taken off when it goes", async () => {
    const clicks: string[] = [];
    let undone = 0;
    contribute({
      kind: "docDecoration",
      id: "clicks",
      owner,
      requires: NO_REQUIREMENT,
      decorate: (root) => {
        const on = (): void => void clicks.push("click");
        root.addEventListener("click", on);
        return () => {
          undone += 1;
          root.removeEventListener("click", on);
        };
      },
    });
    await show("README.md");
    await until(() => target.querySelectorAll(".md-body > *").length > 5, "blocks");
    target.querySelector(".md-body")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(clicks).toEqual(["click"]);
    if (component) await unmount(component);
    component = null;
    expect(undone).toBeGreaterThan(0);
  });
});

describe("the body of a text document", () => {
  it("is drawn by a plugin that asks to, for as long as it asks, and never for a page", async () => {
    const asking = new Box(false);
    contribute({
      kind: "viewer",
      id: "text",
      owner,
      requires: NO_REQUIREMENT,
      claims: () => true,
      view: () => import("../testing/TextBody.svelte"),
    });
    contribute({
      kind: "bodyOverride",
      id: "o",
      owner,
      requires: NO_REQUIREMENT,
      when: () => asking.value,
      view: () => import("../testing/OverrideBody.svelte"),
    });
    await show("src/main.rs");
    // the text viewer (a plugin that is on in this window, or the one of this test)
    const body = (): boolean => target.querySelector(".plain-body, .text") !== null;
    await until(body, "the viewer");
    expect(target.querySelector(".override-body")).toBeNull();
    asking.value = true;
    flushSync();
    await until(() => target.querySelector(".override-body") !== null, "the override");
    expect(target.querySelector(".override-body")?.textContent).toBe("override of src/main.rs");
    expect(body()).toBe(false);
    asking.value = false;
    flushSync();
    await until(body, "the viewer again");
    // a page is the page's own
    asking.value = true;
    await show("README.md");
    await until(() => target.querySelectorAll(".md-body > *").length > 3, "the page");
    expect(target.querySelector(".override-body")).toBeNull();
  });
});
