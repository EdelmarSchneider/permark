// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import type { DocumentView } from "../../api";
import { documents } from "../../core/doc/document.svelte";
import { PluginClient } from "../../core/plugins/client";
import { WindowStore } from "../../core/window.svelte";
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
  await api.openPath(MOCK_FOLDER, "thisTab");
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
      kind: "new",
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

  it("does nothing while another file loads: the passage on screen belongs to the previous page", async () => {
    const { view, text } = page();
    const { store, stop } = await setup({ path: "README.md", view });
    expect(store.begin({ text: "Back to the readme", node: text, endNode: text, rect: null })).toBe(
      false,
    );
    expect(store.draft).toBeNull();
    stop();
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

  it("edits the comment's text and the person's reply, marking them edited", async () => {
    const { store, stop } = await withOne();
    const id = store.all[0]?.id ?? "";
    expect(store.all[0]?.editedMs).toBeNull();
    expect(await store.edit(id, { kind: "comment" }, "  Say it much better ")).toBe(true);
    expect(store.all[0]).toMatchObject({ text: "Say it much better", quote: "Back to the readme" });
    expect(store.all[0]?.editedMs).toEqual(expect.any(Number));
    await store.reply(id, "first thought");
    expect(await store.edit(id, { kind: "reply", index: 0 }, "second thought")).toBe(true);
    expect(store.all[0]?.replies[0]).toMatchObject({ text: "second thought", by: "user" });
    expect(store.all[0]?.replies[0]?.editedMs).toEqual(expect.any(Number));
    expect(await store.edit(id, { kind: "comment" }, "   ")).toBe(false);
    expect(store.all[0]?.text).toBe("Say it much better");
    stop();
  });

  it("tells when an edit is refused (the comment went away meanwhile)", async () => {
    const { api, store, stop, notes } = await withOne();
    const id = store.all[0]?.id ?? "";
    await api.pluginCall("comments.remove", { id });
    expect(await store.edit(id, { kind: "comment" }, "too late")).toBe(false);
    expect(notes.at(-1)).toContain("Couldn't save the comment");
    expect(await store.edit("gone", { kind: "reply", index: 3 }, "x")).toBe(false);
    stop();
  });

  it("opens the field on the text being edited and saves it there", async () => {
    const { store, stop } = await withOne();
    const comment = store.all[0];
    if (!comment) throw new Error("no comment");
    const rect = new DOMRect(1, 2, 3, 4);
    store.beginEdit(comment, { kind: "comment" }, rect);
    expect(store.draft).toEqual({
      kind: "edit",
      id: comment.id,
      target: { kind: "comment" },
      quote: comment.quote,
      text: "Say it better",
      rect,
    });
    expect(await store.save("Better")).toBe(true);
    expect(store.draft).toBeNull();
    expect(store.all[0]?.text).toBe("Better");
    // a reply that is not there: nothing to edit
    store.beginEdit(comment, { kind: "reply", index: 5 }, null);
    expect(store.draft).toBeNull();
    stop();
  });

  it("the mock refuses what the backend refuses", async () => {
    const { api, store, stop } = await withOne();
    const id = store.all[0]?.id ?? "";
    const kind = async (args: { id: string; reply?: number; text: string }) => {
      const result = await api.pluginCall("comments.edit", args);
      if (result.ok) return "ok";
      return result.error.kind === "backend" ? result.error.error.kind : result.error.kind;
    };
    expect(await kind({ id, text: "  " })).toBe("invalidData");
    expect(await kind({ id, text: "é".repeat(2001) })).toBe("tooLarge");
    expect(await kind({ id, reply: 0, text: "x" })).toBe("notFound");
    expect(await kind({ id: "nope", text: "x" })).toBe("notFound");
    expect(await kind({ id, text: "é".repeat(2000) })).toBe("ok");
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
    const draft = store.draft;
    if (draft?.kind !== "new") throw new Error("no draft");
    store.draft = { ...draft, path: "gone.md" };
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

describe("comments removed outside the app", () => {
  const removed = [
    { path: GUIDE, quote: "Back to the readme", text: "Link the install steps", line: 3 },
    { path: "README.md", quote: "Demo", text: "Why this name?", line: 1 },
  ];

  /** A store whose lasting notices go to a window of its own (read in `notices.notices`). */
  async function withAsides() {
    const api = createMockApi();
    await api.openPath(MOCK_FOLDER, "thisTab");
    const notes: string[] = [];
    const notices = new WindowStore(api);
    const store = new CommentsStore(
      new PluginClient(api, () => MOCK_FOLDER),
      () => MOCK_FOLDER,
      (text) => notes.push(text),
      () => Promise.resolve(true),
      notices,
    );
    const stop = store.start();
    await settle();
    return { api, store, stop, notes, notices, organizing: api.probe.plugins.organizing };
  }

  const texts = (store: CommentsStore): string[] => store.all.map((c) => c.text).sort();

  it("shows no line and no notice while nothing was set aside", async () => {
    const { store, stop, notices } = await withAsides();
    expect(store.ready).toBe(true);
    expect(store.lines).toEqual([]);
    expect(notices.notices).toEqual([]);
    stop();
  });

  it("hears an aside, and Restore adds the comments back, says how many, and the line goes", async () => {
    const { store, stop, notes, notices, organizing } = await withAsides();
    organizing.setAside(removed, false);
    await later();
    expect(store.lines).toEqual([{ project: "", folder: "", ids: ["1"], count: 2, big: [] }]);
    // not a large part of the comments: the panel's line is enough
    expect(notices.notices).toEqual([]);
    await store.restore("");
    expect(notes.at(-1)).toBe("Restored 2 comments.");
    expect(texts(store)).toEqual(["Link the install steps", "Why this name?"]);
    expect(store.lines).toEqual([]);
    stop();
  });

  it("a Restore after another window restored them adds nothing and says so", async () => {
    const { api, store, stop, notes, organizing } = await withAsides();
    organizing.setAside(removed, false);
    await later();
    await api.pluginCall("comments.restore", { id: "1" });
    // before this window heard of it
    await store.restore("");
    expect(notes.at(-1)).toBe("Those comments are back already.");
    expect(store.all).toHaveLength(2);
    expect(store.lines).toEqual([]);
    stop();
  });

  it("Dismiss lets the aside go and adds nothing", async () => {
    const { store, stop, organizing } = await withAsides();
    organizing.setAside(removed, false);
    await later();
    await store.dismissAside("");
    expect(store.lines).toEqual([]);
    expect(store.all).toEqual([]);
    stop();
  });

  it("a refused Restore says why and keeps the line", async () => {
    const { store, stop, notes, organizing } = await withAsides();
    organizing.setAside(removed, false);
    organizing.refuseRestore(true);
    await later();
    await store.restore("");
    expect(notes.at(-1)).toBe("Couldn't restore the comments: This isn't available right now.");
    expect(store.all).toEqual([]);
    expect(store.lines).toHaveLength(1);
    stop();
  });

  it("a large loss also gets one lasting notice with Restore, told once; it goes with the aside", async () => {
    const { store, stop, notes, notices, organizing } = await withAsides();
    organizing.setAside(removed, true);
    await later();
    const lasting = () =>
      notices.notices.flatMap((n) => (n.kind === "action" && n.lasting === true ? [n] : []));
    expect(lasting().map((n) => n.text)).toEqual(["2 comments were removed outside PerMark."]);
    expect(lasting()[0]?.actions.map((a) => a.label)).toEqual(["Restore"]);
    // the person dismisses it: another reload does not bring it back
    notices.dismiss(lasting()[0]!.id);
    await store.load();
    expect(lasting()).toEqual([]);
    // a second large loss is told again, in the same single notice
    organizing.setAside([{ path: GUIDE, quote: "Guide", text: "Shorter title", line: 1 }], true);
    await later();
    expect(lasting().map((n) => n.text)).toEqual(["3 comments were removed outside PerMark."]);
    await lasting()[0]!.actions[0]!.run();
    expect(notes.at(-1)).toBe("Restored 3 comments.");
    expect(lasting()).toEqual([]);
    expect(store.lines).toEqual([]);
    stop();
  });

  it("a notice whose aside went elsewhere goes too, and the project going takes everything away", async () => {
    const { api, store, stop, notices, organizing } = await withAsides();
    organizing.setAside(removed, true);
    await later();
    expect(notices.notices).toHaveLength(1);
    await api.pluginCall("comments.dismissAside", { id: "1" });
    await later();
    expect(store.lines).toEqual([]);
    expect(notices.notices).toEqual([]);
    organizing.setAside(removed, true);
    await later();
    expect(notices.notices).toHaveLength(1);
    stop();
    expect(notices.notices).toEqual([]);
    expect(store.lines).toEqual([]);
  });
});
