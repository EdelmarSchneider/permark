// @vitest-environment jsdom
// The Comments tab, the field and the button of a selection, mounted against the mock backend.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER } from "../../api/mock";
import type { CommentView } from "../../api";

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
const { default: CommentRow } = await import("./CommentRow.svelte");
const { exactTime } = await import("../../core/time");
const { language } = await import("../../core/i18n");
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
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
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

  it("offers comments removed outside PerMark back in one line: Restore adds them, Dismiss lets them go", async () => {
    await open();
    component = mount(CommentsPanel, { target });
    flushSync();
    expect(target.querySelector("[data-aside]")).toBeNull();
    const organizing = mock.probe.plugins.organizing;
    const removed = [
      { path: "docs/guide.md", quote: "Back to the readme", text: "Say it better", line: 3 },
    ];
    organizing.setAside(removed, false);
    await until(() => target.querySelector("[data-aside]") !== null, "the line");
    const line = target.querySelector<HTMLElement>("[data-aside]")!;
    expect(line.textContent).toContain("1 comment was removed outside PerMark.");
    expect(line.getAttribute("aria-label")).toBe("1 comment was removed outside PerMark.");
    const button = (label: string): HTMLButtonElement | undefined =>
      Array.from(target.querySelectorAll<HTMLButtonElement>("[data-aside] button")).find(
        (b) => b.textContent?.trim() === label,
      );
    button("Restore")?.click();
    await until(() => target.querySelector("[data-aside]") === null, "restored");
    expect(target.textContent).toContain("Say it better");
    expect(windowStore.notices.some((n) => "text" in n && n.text === "Restored 1 comment.")).toBe(
      true,
    );
    organizing.setAside([{ ...removed[0]!, text: "Other words" }], false);
    await until(() => target.querySelector("[data-aside]") !== null, "the second line");
    button("Dismiss")?.click();
    await until(() => target.querySelector("[data-aside]") === null, "dismissed");
    expect(target.textContent).not.toContain("Other words");
  });

  it("names the folder of an included project on its line, none on the opened folder's", async () => {
    await open();
    component = mount(CommentsPanel, { target });
    flushSync();
    const organizing = mock.probe.plugins.organizing;
    const removed = [{ path: "docs/guide.md", quote: "Back to the readme", text: "One", line: 3 }];
    organizing.setAside(removed, false);
    organizing.setAside([{ ...removed[0]!, text: "Two" }], false, {
      uid: "aaaaaaaaaa",
      folder: "notes/sub a",
    });
    await until(() => target.querySelectorAll("[data-aside]").length === 2, "both lines");
    const own = target.querySelector<HTMLElement>('[data-aside=""]')!;
    const member = target.querySelector<HTMLElement>('[data-aside="aaaaaaaaaa"]')!;
    expect(own.querySelector(".aside-folder")).toBeNull();
    expect(member.querySelector(".aside-folder")?.textContent).toBe("notes/sub a");
    expect(member.getAttribute("aria-label")).toBe(
      "1 comment was removed outside PerMark. notes/sub a",
    );
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
      kind: "new",
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

describe("editing a comment", () => {
  /** The tab and the host of the field, as the window has them. */
  async function mounted(): Promise<{ edit: () => HTMLButtonElement }> {
    await open();
    await addComment("docs/guide.md", "Back to the readme", "Say it beter");
    component = mount(CommentsPanel, { target });
    const host = document.createElement("div");
    document.body.append(host);
    const field = mount(CommentHost, { target: host });
    afterUnmount.push(() => unmount(field));
    flushSync();
    return {
      edit: () => {
        const button = target.querySelector<HTMLButtonElement>('.actions [aria-label="Edit"]');
        if (!button) throw new Error("no Edit");
        return button;
      },
    };
  }
  const afterUnmount: (() => unknown)[] = [];
  afterEach(async () => {
    for (const run of afterUnmount.splice(0)) await run();
  });

  function box(): HTMLTextAreaElement {
    const found = document.querySelector<HTMLTextAreaElement>('[role="dialog"] textarea');
    if (!found) throw new Error("no field");
    return found;
  }

  /** As a pointer or a key does: the button has the focus when it runs. */
  function activate(button: HTMLButtonElement): void {
    button.focus();
    button.click();
    flushSync();
  }

  it("opens the field prefilled with the caret at the end, saves with Ctrl+Enter, focus back on Edit", async () => {
    const { edit } = await mounted();
    activate(edit());
    const field = box();
    expect(field.value).toBe("Say it beter");
    expect(document.activeElement).toBe(field);
    expect([field.selectionStart, field.selectionEnd]).toEqual([12, 12]);
    field.value = "Say it better";
    field.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    press(field, "Enter", { ctrlKey: true });
    await until(() => comments.all[0]?.text === "Say it better", "saved");
    flushSync();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(edit());
    expect(target.querySelector(".edited")?.textContent).toBe("edited");
  });

  it("leaves the text as it was on Escape or Cancel, focus back on Edit", async () => {
    const { edit } = await mounted();
    activate(edit());
    box().value = "something else";
    box().dispatchEvent(new Event("input", { bubbles: true }));
    press(box(), "Escape");
    flushSync();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(edit());
    activate(edit());
    const cancel = Array.from(
      document.querySelectorAll<HTMLButtonElement>('[role="dialog"] button'),
    ).find((b) => b.textContent?.trim() === "Cancel");
    cancel?.click();
    flushSync();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(edit());
    expect(comments.all[0]?.text).toBe("Say it beter");
    expect(comments.all[0]?.editedMs).toBeNull();
    expect(target.querySelector(".edited")).toBeNull();
  });

  it("starting another edit while one is open shows the other comment's text, never the first one's", async () => {
    const { edit } = await mounted();
    activate(edit());
    box().value = "typed for the first comment";
    box().dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    const first = comments.all[0];
    if (!first) throw new Error("no comment");
    comments.beginEdit(
      { ...first, id: "another", text: "The other comment" },
      { kind: "comment" },
      null,
    );
    flushSync();
    expect(box().value).toBe("The other comment");
    press(box(), "Escape");
    flushSync();
    expect(comments.all[0]?.text).toBe("Say it beter");
  });

  it("edits the person's reply, never the agent's", async () => {
    const { edit } = await mounted();
    void edit;
    const id = comments.all[0]?.id ?? "";
    await comments.reply(id, "my reply");
    flushSync();
    const replyEdit = target.querySelector<HTMLButtonElement>('.reply [aria-label="Edit"]');
    if (!replyEdit) throw new Error("no Edit on the reply");
    activate(replyEdit);
    expect(box().value).toBe("my reply");
    box().value = "my better reply";
    box().dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    document.querySelector<HTMLButtonElement>('[role="dialog"] .btn--primary')?.click();
    await until(() => comments.all[0]?.replies[0]?.text === "my better reply", "saved");
    flushSync();
    expect(document.activeElement).toBe(
      target.querySelector<HTMLButtonElement>('.reply [aria-label="Edit"]'),
    );
  });
});

describe("one comment row", () => {
  const view = (over: Partial<CommentView> = {}): CommentView => ({
    id: "c1",
    path: "docs/guide.md",
    text: "words",
    quote: "Back to the readme",
    headingPath: [],
    line: 3,
    endLine: 3,
    createdMs: 1_700_000_000_000,
    editedMs: null,
    state: "anchored",
    done: false,
    doneMs: null,
    doneBy: null,
    reply: null,
    replies: [],
    ...over,
  });

  it("offers Edit on the comment and the person's replies only", () => {
    component = mount(CommentRow, {
      target,
      props: {
        comment: view({
          done: true,
          doneMs: 1_700_000_100_000,
          doneBy: "agent",
          reply: "fixed it",
          replies: [
            { atMs: 1, by: "user", text: "mine", editedMs: null },
            { atMs: 2, by: "agent", text: "theirs", editedMs: null },
          ],
        }),
      },
    });
    flushSync();
    expect(target.querySelectorAll('[aria-label="Edit"]')).toHaveLength(2);
    const replies = Array.from(target.querySelectorAll(".reply"));
    expect(replies.map((r) => r.querySelector('[aria-label="Edit"]') !== null)).toEqual([
      false,
      true,
      false,
    ]);
  });

  it("shows edited after the time, the exact time as its tooltip", () => {
    const at = 1_700_000_500_000;
    component = mount(CommentRow, {
      target,
      props: {
        comment: view({
          editedMs: at,
          replies: [{ atMs: 1, by: "user", text: "mine", editedMs: at + 1 }],
        }),
      },
    });
    flushSync();
    const marks = target.querySelectorAll<HTMLElement>(".edited");
    expect(marks).toHaveLength(2);
    expect(marks[0]?.closest(".when")).not.toBeNull();
    expect(marks[0]?.textContent).toBe("edited");
    expect(marks[0]?.title).toBe(exactTime(at, language.locale));
    expect(marks[1]?.title).toBe(exactTime(at + 1, language.locale));
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
