// @vitest-environment jsdom
// The Comments tab, the field and the button of a selection, mounted against the mock backend.
import { flushSync, mount, unmount } from "svelte";
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
const { default: CommentsPanel } = await import("./CommentsPanel.svelte");
const { default: CommentField } = await import("./CommentField.svelte");
const { default: CommentHost } = await import("./CommentHost.svelte");
const { comments } = await import("./store.svelte");
const { windowStore } = await import("../../core/window.svelte");
const { documents } = await import("../../core/doc/document.svelte");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const GUIDE_TEXT = mock.probe.project.files.get("docs/guide.md") ?? "";

let target: HTMLElement;
let component: Record<string, unknown> | null = null;
let stop: (() => void) | null = null;
let started = false;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 3000, interval: 10 });
}

function press(el: Element, key: string, init: KeyboardEventInit = {}): void {
  el.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init }));
}

async function open(): Promise<void> {
  await mock.openPath(MOCK_FOLDER, "thisTab");
  if (!started) {
    await windowStore.start();
    started = true;
  }
  stop = comments.start();
  await until(() => comments.ready, "the comments are loaded");
}

async function addComment(path: string, quote: string, text: string): Promise<string> {
  const result = await mock.pluginCall("comments.add", {
    path,
    quote,
    text,
    headingPath: ["Guide"],
    line: 3,
    endLine: 3,
  });
  if (!result.ok) throw new Error("add");
  await comments.load();
  return result.value.id;
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
  mock.probe.plugins.organizing.reset();
  mock.probe.project.add("docs/guide.md", GUIDE_TEXT);
  comments.cancel();
  vi.restoreAllMocks();
});

describe("the Comments tab", () => {
  it("says there is nothing yet, and how to begin", async () => {
    await open();
    component = mount(CommentsPanel, { target });
    flushSync();
    expect(target.textContent).toContain("No comments yet");
    expect(target.textContent).toContain("press C");
    expect(target.querySelector(".send")).toBeNull();
  });

  it("groups the open comments by document, with the passage, the text and the place", async () => {
    await open();
    await addComment("docs/guide.md", "Back to the readme", "Say it better");
    await addComment("README.md", "Demo", "Why this name?");
    component = mount(CommentsPanel, { target });
    flushSync();
    const titles = Array.from(target.querySelectorAll("h3 .name")).map((n) => n.textContent);
    expect(titles).toEqual(["guide.md", "README.md"]);
    expect(target.textContent).toContain("Say it better");
    expect(target.textContent).toContain("Back to the readme");
    expect(target.querySelector(".count")?.textContent).toBe("2");
    expect(target.querySelector(".send")?.textContent).toContain("Send all (2)");
  });

  it("marks done into a folded list, opens again, replies and deletes", async () => {
    await open();
    await addComment("docs/guide.md", "Back to the readme", "Say it better");
    component = mount(CommentsPanel, { target });
    flushSync();
    const click = (label: string): void => {
      target.querySelector<HTMLElement>(`[aria-label="${label}"]`)?.click();
    };
    click("Mark done");
    await until(() => target.querySelector("details") !== null, "the done list");
    expect(target.querySelector("details summary")?.textContent).toContain("1 done");
    expect(target.textContent).toContain("Every comment is done.");
    target.querySelector<HTMLDetailsElement>("details")!.open = true;
    flushSync();
    click("Mark as not done");
    await until(() => target.querySelector("details") === null, "open again");
    click("Reply");
    flushSync();
    const reply = target.querySelector<HTMLTextAreaElement>("textarea");
    if (!reply) throw new Error("no reply field");
    reply.value = "thanks";
    reply.dispatchEvent(new Event("input", { bubbles: true }));
    press(reply, "Enter");
    await until(() => target.textContent?.includes("thanks") === true, "the reply");
    // delete asks first
    click("Delete comment");
    flushSync();
    expect(comments.all).toHaveLength(1);
    target.querySelector<HTMLElement>(".confirm")?.click();
    await until(() => comments.all.length === 0, "deleted");
  });

  it("says when the passage changed or the document is gone", async () => {
    await open();
    await addComment("docs/guide.md", "text that is not there", "x");
    component = mount(CommentsPanel, { target });
    flushSync();
    expect(target.textContent).toContain("Passage changed");
    mock.probe.project.files.delete("docs/guide.md");
    await comments.load();
    flushSync();
    expect(target.textContent).toContain("Document missing");
    expect(target.querySelector<HTMLButtonElement>(".where")?.disabled).toBe(true);
  });

  it("scrolls to and marks the comment a click in the margin asked for", async () => {
    await open();
    const id = await addComment("docs/guide.md", "Back to the readme", "Say it better");
    component = mount(CommentsPanel, { target });
    flushSync();
    comments.show(id);
    await until(() => target.querySelector(".comment.marked") !== null, "marked");
    expect(target.querySelector(".comment.marked")?.getAttribute("data-comment")).toBe(id);
  });
});

describe("writing a comment", () => {
  function draft(): void {
    comments.draft = {
      path: "docs/guide.md",
      quote: "Back to the readme",
      headingPath: ["Guide"],
      line: 3,
      endLine: 3,
      rect: new DOMRect(40, 120, 200, 18),
    };
  }

  it("saves with Enter, keeps a new line for Shift+Enter, and cancels with Escape", async () => {
    await open();
    draft();
    component = mount(CommentField, { target });
    flushSync();
    const box = target.querySelector<HTMLTextAreaElement>("textarea");
    if (!box) throw new Error("no field");
    expect(target.querySelector(".quote")?.textContent).toBe("Back to the readme");
    expect(target.querySelector<HTMLButtonElement>(".btn--primary")?.disabled).toBe(true);
    box.value = "Say it better";
    box.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    press(box, "Enter", { shiftKey: true });
    expect(comments.all).toHaveLength(0);
    press(box, "Enter");
    await until(() => comments.all.length === 1, "saved");
    expect(comments.draft).toBeNull();
    draft();
    flushSync();
    press(target.querySelector<HTMLElement>('[role="dialog"]')!, "Escape");
    expect(comments.draft).toBeNull();
  });

  it("closes on a click elsewhere while it is empty, and stays while something is written", async () => {
    await open();
    draft();
    component = mount(CommentField, { target });
    flushSync();
    const box = target.querySelector<HTMLTextAreaElement>("textarea");
    if (!box) throw new Error("no field");
    box.value = "a half-written thought";
    box.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    expect(comments.draft).not.toBeNull();
    box.value = "";
    box.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    expect(comments.draft).toBeNull();
  });
});

describe("the button of a selection", () => {
  it("appears next to a selected passage of the page and starts the comment", async () => {
    await open();
    document.body.insertAdjacentHTML(
      "beforeend",
      '<div id="page"><p data-b="p1">Back to the readme.</p></div>',
    );
    const page = document.getElementById("page");
    const text = page?.querySelector("p")?.firstChild;
    if (!page || !text) throw new Error("page");
    vi.spyOn(documents, "path", "get").mockReturnValue("docs/guide.md");
    vi.spyOn(documents.host, "container", "get").mockReturnValue(page);
    vi.spyOn(documents, "view", "get").mockReturnValue({
      path: "docs/guide.md",
      body: {
        kind: "markdown",
        blocks: [{ id: "p1", kind: "p", lineStart: 3, lineEnd: 3, html: null }],
        outline: [],
      },
    } as never);
    // a page has a place on screen; jsdom has no layout
    Range.prototype.getBoundingClientRect = () => new DOMRect(40, 120, 60, 18);
    component = mount(CommentHost, { target });
    flushSync();
    expect(target.querySelector(".add")).toBeNull();
    const range = document.createRange();
    range.setStart(text, 0);
    range.setEnd(text, 4);
    window.getSelection()?.removeAllRanges();
    window.getSelection()?.addRange(range);
    document.dispatchEvent(new Event("selectionchange"));
    flushSync();
    const button = target.querySelector<HTMLElement>(".add");
    expect(button?.textContent).toContain("Comment");
    button?.click();
    flushSync();
    expect(comments.draft).toMatchObject({ quote: "Back", line: 3, endLine: 3 });
    expect(target.querySelector(".add")).toBeNull();
    expect(target.querySelector("textarea")).not.toBeNull();
  });
});
