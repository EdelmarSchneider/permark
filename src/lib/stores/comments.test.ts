// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// The backend is replaced: these tests are about what the store asks it, when, and what it keeps meanwhile.
const fake = vi.hoisted(() => ({
  addComment: vi.fn(),
  listComments: vi.fn(),
  setCommentDone: vi.fn(),
  deleteComment: vi.fn(),
}));
vi.mock("../api", () => ({ api: { ...fake, on: async () => () => {} } }));

// The clipboard is replaced too: these tests are about what lands on it and what the person is told.
const clipboard = vi.hoisted(() => ({ copyText: vi.fn(async (_text: string) => true) }));
vi.mock("../platform", () => clipboard);

import { bridge } from "../bridge";
import type { CommentTarget } from "../dom/commentDom";
import type { DocumentView, FsBatch, Comment, VaultInfo } from "../types";
import { docs } from "./docs.svelte";
import { CommentsStore, REFRESH_DELAY_MS, REMEMBER_MS } from "./comments.svelte";
import { toast } from "./toast.svelte";
import { vault } from "./vault.svelte";

let counter = 0;
const comment = (patch: Partial<Comment> = {}): Comment => ({
  id: `n${++counter}`,
  path: "docs/plan.md",
  text: "A comment",
  quote: "a passage",
  headingPath: ["Plan"],
  line: 3,
  endLine: 3,
  createdMs: 1000 + counter,
  state: "anchored",
  done: false,
  ...patch,
});

const target = (patch: Partial<CommentTarget> = {}): CommentTarget => ({
  quote: "ship it on Friday",
  startLine: 7,
  endLine: 7,
  headingPath: ["Plan", "Rollout"],
  rect: new DOMRect(10, 20, 100, 16),
  range: document.createRange(),
  ...patch,
});

const batch = (...paths: string[]): FsBatch => ({
  changes: paths.map((path) => ({ kind: "modified" as const, path, atMs: 1 })),
  bulk: null,
  rescan: false,
});

let clockMs = 0;
let store: CommentsStore;
let selection: CommentTarget | null;

function openDocument(path: string | null, kind: DocumentView["kind"] = "markdown"): void {
  docs.current = path === null ? null : ({ path, kind } as DocumentView);
}

beforeEach(() => {
  vi.useFakeTimers();
  for (const fn of Object.values(fake)) fn.mockReset();
  fake.listComments.mockResolvedValue([]);
  clipboard.copyText.mockReset();
  clipboard.copyText.mockResolvedValue(true);
  toast.clear();
  clockMs = 1_000_000;
  store = new CommentsStore(() => clockMs);
  selection = target();
  bridge.commentTarget = () => selection;
  openDocument("docs/plan.md");
  docs.editing = false;
  vault.info = { mode: "ide" } as VaultInfo;
});

afterEach(() => {
  bridge.commentTarget = undefined;
  openDocument(null);
  vault.info = null;
  vi.useRealTimers();
});

describe("the list", () => {
  it("is what the backend says, asked for when the project opens", async () => {
    const list = [comment({ line: 2 }), comment({ done: true })];
    fake.listComments.mockResolvedValue(list);
    await store.refresh();
    expect(store.all).toEqual(list);
    expect(store.count).toBe(1);
    expect(store.groups.map((g) => g.path)).toEqual(["docs/plan.md"]);
    expect(store.done).toHaveLength(1);
  });

  it("is not asked for in a single file, which has no project", async () => {
    vault.info = { mode: "simple" } as VaultInfo;
    await store.refresh();
    expect(fake.listComments).not.toHaveBeenCalled();
  });

  it("is not overwritten by an answer that a newer question has overtaken", async () => {
    let answer: (comments: Comment[]) => void = () => {};
    fake.listComments.mockReturnValueOnce(new Promise<Comment[]>((resolve) => (answer = resolve)));
    const old = store.refresh();
    fake.listComments.mockResolvedValueOnce([comment({ text: "new" })]);
    await store.refresh();
    answer([comment({ text: "old" })]);
    await old;
    expect(store.all.map((n) => n.text)).toEqual(["new"]);
  });

  it("is forgotten when the project changes, and an answer on its way is dropped", async () => {
    let answer: (comments: Comment[]) => void = () => {};
    fake.listComments.mockReturnValueOnce(new Promise<Comment[]>((resolve) => (answer = resolve)));
    const pending = store.refresh();
    store.reset();
    answer([comment()]);
    await pending;
    expect(store.all).toEqual([]);
  });

  it("keeps what it had when the question fails", async () => {
    fake.listComments.mockResolvedValue([comment()]);
    await store.refresh();
    fake.listComments.mockRejectedValue(new Error("no"));
    await store.refresh();
    expect(store.all).toHaveLength(1);
  });
});

describe("keeping up with the documents", () => {
  beforeEach(async () => {
    fake.listComments.mockResolvedValue([comment({ path: "docs/plan.md" })]);
    await store.refresh();
    fake.listComments.mockClear();
  });

  it("asks again, once, a moment after a document with comments is drawn again", async () => {
    store.documentChanged();
    store.documentChanged();
    expect(fake.listComments).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(REFRESH_DELAY_MS);
    expect(fake.listComments).toHaveBeenCalledTimes(1);
  });

  it("asks again when files with comments change on disk, and not for other files", async () => {
    store.filesChanged(batch("docs/other.md"));
    await vi.advanceTimersByTimeAsync(REFRESH_DELAY_MS * 2);
    expect(fake.listComments).not.toHaveBeenCalled();
    store.filesChanged(batch("docs/plan.md"));
    await vi.advanceTimersByTimeAsync(REFRESH_DELAY_MS);
    expect(fake.listComments).toHaveBeenCalledTimes(1);
  });

  it("asks again for a rescan or a bulk change, which could have touched anything", async () => {
    store.filesChanged({ changes: [], bulk: null, rescan: true });
    await vi.advanceTimersByTimeAsync(REFRESH_DELAY_MS);
    expect(fake.listComments).toHaveBeenCalledTimes(1);
    store.filesChanged({
      changes: [],
      bulk: { count: 900, reason: "gitOperation", paths: [] },
      rescan: false,
    });
    await vi.advanceTimersByTimeAsync(REFRESH_DELAY_MS);
    expect(fake.listComments).toHaveBeenCalledTimes(2);
  });

  it("asks at once when the backend says the comments changed", async () => {
    store.onChanged();
    await vi.advanceTimersByTimeAsync(0);
    expect(fake.listComments).toHaveBeenCalledTimes(1);
  });

  it("does not ask while there are no comments: no document can have gained one", async () => {
    store.reset();
    store.documentChanged();
    store.filesChanged(batch("docs/plan.md"));
    await vi.advanceTimersByTimeAsync(REFRESH_DELAY_MS * 2);
    expect(fake.listComments).not.toHaveBeenCalled();
  });
});

describe("writing a comment", () => {
  it("opens the field on the selection, with the document it is in", () => {
    expect(store.begin()).toBe(true);
    expect(store.draft?.path).toBe("docs/plan.md");
    expect(store.draft?.target.quote).toBe("ship it on Friday");
  });

  it("says to select something first when nothing is selected", () => {
    selection = null;
    expect(store.begin()).toBe(false);
    expect(store.draft).toBeNull();
    expect(toast.current?.type === "message" && toast.current.text).toBe(
      "Select a passage in the document first, then add the comment.",
    );
  });

  it("does nothing outside a Markdown document of a project", () => {
    openDocument("data.json", "json" as DocumentView["kind"]);
    expect(store.begin()).toBe(false);
    openDocument("docs/plan.md");
    docs.editing = true;
    expect(store.begin()).toBe(false);
    docs.editing = false;
    vault.info = { mode: "simple" } as VaultInfo;
    expect(store.begin()).toBe(false);
    openDocument(null);
    vault.info = { mode: "ide" } as VaultInfo;
    expect(store.begin()).toBe(false);
    expect(toast.current).toBeNull();
  });

  it("uses the selection seen a moment ago when something took it away", () => {
    store.remember();
    selection = null;
    expect(store.begin()).toBe(true);
    expect(store.draft?.target.quote).toBe("ship it on Friday");
  });

  it("forgets that selection after a minute, and after another document was opened", () => {
    store.remember();
    selection = null;
    clockMs += REMEMBER_MS;
    expect(store.begin()).toBe(false);

    store.remember();
    openDocument("docs/other.md");
    expect(store.begin()).toBe(false);
  });

  it("prefers the selection on screen to the one remembered", () => {
    store.remember();
    selection = target({ quote: "another passage" });
    store.begin();
    expect(store.draft?.target.quote).toBe("another passage");
  });

  it("saves the comment with the passage as the view knew it, then closes the field and asks for the list", async () => {
    store.begin();
    fake.addComment.mockResolvedValue(comment());
    fake.listComments.mockResolvedValue([comment({ text: "Too vague." })]);
    expect(await store.save("  Too vague.  ")).toBe(true);
    expect(fake.addComment).toHaveBeenCalledWith({
      path: "docs/plan.md",
      quote: "ship it on Friday",
      text: "Too vague.",
      headingPath: ["Plan", "Rollout"],
      line: 7,
      endLine: 7,
    });
    expect(store.draft).toBeNull();
    expect(store.all.map((n) => n.text)).toEqual(["Too vague."]);
    expect(store.saving).toBe(false);
  });

  it("does not save an empty comment", async () => {
    store.begin();
    expect(await store.save("   ")).toBe(false);
    expect(fake.addComment).not.toHaveBeenCalled();
    expect(store.draft).not.toBeNull();
  });

  it("keeps the field open, with a message, when the comment cannot be saved", async () => {
    store.begin();
    fake.addComment.mockRejectedValue({ kind: "emptyComment", message: "x" });
    expect(await store.save("Words")).toBe(false);
    expect(store.draft).not.toBeNull();
    expect(store.saving).toBe(false);
    expect(toast.current?.type === "message" && toast.current.text).toContain(
      "Couldn't save the comment",
    );
  });

  it("saves once while a save is on its way", async () => {
    store.begin();
    let finish: (n: Comment) => void = () => {};
    fake.addComment.mockReturnValue(new Promise<Comment>((resolve) => (finish = resolve)));
    const first = store.save("Words");
    expect(await store.save("Words")).toBe(false);
    finish(comment());
    await first;
    expect(fake.addComment).toHaveBeenCalledTimes(1);
  });

  it("closes without a comment", () => {
    store.begin();
    store.cancel();
    expect(store.draft).toBeNull();
    store.cancel();
  });
});

describe("the list's own actions", () => {
  it("checks a comment off and keeps what the backend answered", async () => {
    const open = comment();
    fake.listComments.mockResolvedValue([open]);
    await store.refresh();
    fake.setCommentDone.mockResolvedValue({ ...open, done: true, doneMs: 5, doneBy: "person" });
    await store.setDone(open.id, true);
    expect(fake.setCommentDone).toHaveBeenCalledWith(open.id, true);
    expect(store.count).toBe(0);
    expect(store.done.map((n) => n.id)).toEqual([open.id]);
  });

  it("asks the backend again when it could not change a comment", async () => {
    const open = comment();
    fake.listComments.mockResolvedValue([open]);
    await store.refresh();
    fake.setCommentDone.mockRejectedValue({ kind: "commentNotFound", message: "gone" });
    fake.listComments.mockClear();
    await store.setDone(open.id, true);
    expect(fake.listComments).toHaveBeenCalledTimes(1);
    expect(toast.current?.type).toBe("message");
  });

  it("deletes a comment", async () => {
    const [one, two] = [comment(), comment()];
    fake.listComments.mockResolvedValue([one, two]);
    await store.refresh();
    fake.deleteComment.mockResolvedValue(undefined);
    await store.remove(one.id);
    expect(fake.deleteComment).toHaveBeenCalledWith(one.id);
    expect(store.all.map((n) => n.id)).toEqual([two.id]);
  });

  it("asks the panel to show a comment, again on every request", () => {
    store.show("c1");
    const first = store.focus;
    store.show("c1");
    expect(store.focus?.id).toBe("c1");
    expect(store.focus?.n).toBeGreaterThan(first?.n ?? 0);
  });
});

describe("sending to an agent", () => {
  it("puts one comment on the clipboard as a prompt and says so", async () => {
    const open = comment({ text: "Too vague." });
    fake.listComments.mockResolvedValue([open]);
    await store.refresh();
    await store.sendToAgent(open.id);
    expect(clipboard.copyText).toHaveBeenCalledTimes(1);
    expect(clipboard.copyText.mock.calls[0][0]).toContain("Too vague.");
    expect(toast.current?.type === "message" && toast.current.tone).toBe("info");
  });

  it("does not send a comment that is done", async () => {
    const done = comment({ done: true, doneMs: 5 });
    fake.listComments.mockResolvedValue([done]);
    await store.refresh();
    await store.sendToAgent(done.id);
    expect(clipboard.copyText).not.toHaveBeenCalled();
  });

  it("sends every open comment grouped by document, and not the done ones", async () => {
    const list = [
      comment({ text: "first one" }),
      comment({ path: "docs/other.md", text: "second one" }),
      comment({ done: true, doneMs: 5, text: "finished one" }),
    ];
    fake.listComments.mockResolvedValue(list);
    await store.refresh();
    await store.sendAllToAgent();
    expect(clipboard.copyText).toHaveBeenCalledTimes(1);
    const text = clipboard.copyText.mock.calls[0][0];
    expect(text).toContain("## docs/plan.md");
    expect(text).toContain("## docs/other.md");
    expect(text).toContain("first one");
    expect(text).toContain("second one");
    expect(text).not.toContain("finished one");
  });

  it("says what every failed copy says when the clipboard refuses", async () => {
    clipboard.copyText.mockResolvedValueOnce(false);
    fake.listComments.mockResolvedValue([comment()]);
    await store.refresh();
    await store.sendAllToAgent();
    expect(toast.current?.type === "message" && toast.current.tone).toBe("warn");
  });

  it("sends nothing when no comment is open", async () => {
    fake.listComments.mockResolvedValue([comment({ done: true, doneMs: 5 })]);
    await store.refresh();
    await store.sendAllToAgent();
    expect(clipboard.copyText).not.toHaveBeenCalled();
  });
});
