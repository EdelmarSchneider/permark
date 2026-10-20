// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import type { DocumentView } from "../../api";
import { documents } from "../../core/doc/document.svelte";
import { PluginClient } from "../../core/plugins/client";
import { CommentsStore, RELOAD_MS } from "./store.svelte";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 5; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

async function later(ms = RELOAD_MS + 20): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, ms));
  await settle();
}

const GUIDE = "docs/guide.md";

/** A page with three blocks (a heading and two paragraphs), as the document view knows them. */
function page(): { view: DocumentView; text: Text; other: Text } {
  document.body.innerHTML =
    '<div id="page"><h1 data-b="h">Guide</h1><p data-b="p1">Back to the readme.</p><p data-b="p2">More words here.</p></div>';
  const text = document.querySelector('[data-b="p1"]')?.firstChild;
  const other = document.querySelector('[data-b="p2"]')?.firstChild;
  if (!(text instanceof Text) || !(other instanceof Text)) throw new Error("page");
  const view = {
    path: GUIDE,
    body: {
      kind: "markdown",
      blocks: [
        { id: "h", kind: "h1", lineStart: 1, lineEnd: 1, html: null },
        { id: "p1", kind: "p", lineStart: 3, lineEnd: 3, html: null },
        { id: "p2", kind: "p", lineStart: 5, lineEnd: 6, html: null },
      ],
      outline: [{ label: "Guide", level: 1, id: "guide", line: 1, block: "h" }],
    },
  } as unknown as DocumentView;
  return { view, text, other };
}

async function setup(shown: { path: string | null; view: DocumentView | null }) {
  const api = createMockApi();
  await api.openPath(MOCK_FOLDER, "thisWindow");
  vi.spyOn(documents, "path", "get").mockReturnValue(shown.path);
  vi.spyOn(documents, "view", "get").mockReturnValue(shown.view);
  const notes: string[] = [];
  const copied: string[] = [];
  const store = new CommentsStore(
    new PluginClient(api, () => MOCK_FOLDER),
    () => MOCK_FOLDER,
    (text) => notes.push(text),
    (text) => {
      copied.push(text);
      return Promise.resolve(true);
    },
  );
  const stop = store.start();
  await settle();
  return { api, store, stop, notes, copied };
}

afterEach(() => {
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

describe("starting a comment", () => {
  it("takes the passage, its lines and the headings above it from the page", async () => {
    const { view, text, other } = page();
    const { store, stop } = await setup({ path: GUIDE, view });
    const rect = new DOMRect(10, 20, 100, 16);
    expect(store.begin({ text: " Back to the readme. ", node: text, endNode: other, rect })).toBe(
      true,
    );
    expect(store.draft).toEqual({
      path: GUIDE,
      quote: "Back to the readme.",
      headingPath: ["Guide"],
      line: 3,
      endLine: 6,
      rect,
    });
    store.cancel();
    expect(store.draft).toBeNull();
    stop();
  });

  it("does nothing without a passage, a document or a Markdown page", async () => {
    const { view, text } = page();
    const sel = { text: "x", node: text, endNode: text, rect: null };
    const none = await setup({ path: null, view: null });
    expect(none.store.begin(sel)).toBe(false);
    none.stop();
    vi.restoreAllMocks();
    const blank = await setup({ path: GUIDE, view });
    expect(blank.store.begin({ ...sel, text: "  " })).toBe(false);
    expect(blank.store.draft).toBeNull();
    blank.stop();
    vi.restoreAllMocks();
    const media = await setup({
      path: "logo.png",
      view: { path: "logo.png", body: { kind: "media" } } as unknown as DocumentView,
    });
    expect(media.store.begin(sel)).toBe(false);
    media.stop();
  });
});

describe("the comments", () => {
  async function withOne() {
    const { view, text, other } = page();
    const env = await setup({ path: GUIDE, view });
    env.store.begin({ text: "Back to the readme", node: text, endNode: text, rect: null });
    const saved = await env.store.save("  Say it better ");
    expect(saved).toBe(true);
    void other;
    return env;
  }

  it("saves a comment, lists it, gives its document a bar and a count", async () => {
    const { store, stop } = await withOne();
    expect(store.draft).toBeNull();
    expect(store.count).toBe(1);
    expect(store.groups.map((g) => g.path)).toEqual([GUIDE]);
    expect(store.groups[0]?.comments[0]).toMatchObject({
      text: "Say it better",
      quote: "Back to the readme",
      state: "anchored",
    });
    expect(store.bars(GUIDE)).toHaveLength(1);
    expect(store.bars("README.md")).toEqual([]);
    expect(store.badge(GUIDE, false)).toMatchObject({ text: "1", tone: "muted" });
    expect(store.badge(GUIDE, true)).toBeNull();
    expect(store.badge("README.md", false)).toBeNull();
    stop();
  });

  it("marks done and opens again, replies, deletes", async () => {
    const { store, stop } = await withOne();
    const id = store.all[0]?.id ?? "";
    await store.setDone(id, true);
    expect(store.count).toBe(0);
    expect(store.done.map((c) => c.id)).toEqual([id]);
    expect(store.bars(GUIDE)).toEqual([]);
    await store.setDone(id, false);
    expect(store.count).toBe(1);
    expect(await store.reply(id, "  thanks ")).toBe(true);
    expect(store.all[0]?.replies.map((r) => r.text)).toEqual(["thanks"]);
    expect(await store.reply(id, "   ")).toBe(false);
    await store.remove(id);
    expect(store.all).toEqual([]);
    stop();
  });

  it("refuses an empty comment and keeps the draft", async () => {
    const { view, text } = page();
    const { store, stop } = await setup({ path: GUIDE, view });
    store.begin({ text: "Back to the readme", node: text, endNode: text, rect: null });
    expect(await store.save("   ")).toBe(false);
    expect(store.draft).not.toBeNull();
    stop();
  });

  it("tells when the comment could not be saved", async () => {
    const { view, text } = page();
    const { store, stop, notes } = await setup({ path: GUIDE, view });
    store.begin({ text: "Back to the readme", node: text, endNode: text, rect: null });
    // the document went away meanwhile
    store.draft = { ...(store.draft as NonNullable<typeof store.draft>), path: "gone.md" };
    expect(await store.save("text")).toBe(false);
    expect(notes[0]).toContain("Couldn't save the comment");
    expect(store.draft).not.toBeNull();
    stop();
  });

  it("follows the backend when it says the comments changed", async () => {
    const { api, store, stop } = await withOne();
    expect(store.count).toBe(1);
    // an agent resolves it elsewhere: the next list says so
    const id = store.all[0]?.id ?? "";
    await api.pluginCall("comments.setDone", { id, done: true });
    await later();
    expect(store.count).toBe(0);
    stop();
  });
});

describe("send to agent", () => {
  it("copies a prompt for one comment or for all the open ones", async () => {
    const { view, text } = page();
    const { store, stop, notes, copied } = await setup({ path: GUIDE, view });
    for (const quote of ["Back to the readme", "Guide"]) {
      store.begin({ text: quote, node: text, endNode: text, rect: null });
      await store.save(`about ${quote}`);
    }
    const first = store.all[0];
    if (!first) throw new Error("no comment");
    await store.sendToAgent(first.id);
    expect(copied[0]).toContain(`Please address this comment on ${GUIDE}`);
    expect(notes.at(-1)).toContain("Copied");
    await store.sendAllToAgent();
    expect(copied[1]).toContain(`## ${GUIDE}`);
    expect(copied[1]).toContain("about Guide");
    // a done comment is not sent
    await store.setDone(first.id, true);
    await store.sendToAgent(first.id);
    expect(copied).toHaveLength(2);
    stop();
  });

  it("says when the clipboard refuses", async () => {
    const { view, text } = page();
    const env = await setup({ path: GUIDE, view });
    const refusing = new CommentsStore(
      new PluginClient(env.api, () => MOCK_FOLDER),
      () => MOCK_FOLDER,
      (t) => env.notes.push(t),
      () => Promise.resolve(false),
    );
    const stop = refusing.start();
    await settle();
    env.store.begin({ text: "Guide", node: text, endNode: text, rect: null });
    await env.store.save("x");
    await refusing.load();
    await refusing.sendAllToAgent();
    expect(env.notes.at(-1)).toContain("Couldn't copy");
    stop();
    env.stop();
  });
});

describe("going to the passage", () => {
  it("opens the document at the line, or says it is gone", async () => {
    const { view, text } = page();
    const { store, stop, notes } = await setup({ path: GUIDE, view });
    store.begin({ text: "Guide", node: text, endNode: text, rect: null });
    await store.save("x");
    const navigate = vi.spyOn(documents, "navigate").mockImplementation(() => {});
    const comment = store.all[0];
    if (!comment) throw new Error("no comment");
    store.jump(comment);
    expect(navigate).toHaveBeenCalledWith(GUIDE, null, comment.line);
    store.jump({ ...comment, state: "documentMissing" });
    expect(navigate).toHaveBeenCalledTimes(1);
    expect(notes.at(-1)).toContain(GUIDE);
    store.show(comment.id);
    expect(store.focus?.id).toBe(comment.id);
    stop();
  });
});

describe("the project goes", () => {
  it("forgets the comments and the draft", async () => {
    const { view, text } = page();
    const { store, stop } = await setup({ path: GUIDE, view });
    store.begin({ text: "Guide", node: text, endNode: text, rect: null });
    stop();
    expect(store.draft).toBeNull();
    expect(store.all).toEqual([]);
    expect(store.ready).toBe(false);
  });
});
