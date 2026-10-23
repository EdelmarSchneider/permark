// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Api, PluginMethod, PluginMethods, Result } from "../../api";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { PluginClient } from "../../core/plugins/client";
import { Box } from "../../core/testing/box.svelte";
import { ReviewStore, SEEN_MS, kindOf, type ReviewKind, type ReviewNotice } from "./store.svelte";
import type { DocumentView } from "../../api";
import type { Reason } from "../../core/conditions/available";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 6; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

type Shown = { path: string | null; hash: string | null; kind: ReviewKind };
const GUIDE = "docs/guide.md";

async function setup(
  options: {
    conflictOn?: PluginMethod;
    failOn?: { method: PluginMethod; basis: string };
    shown?: Shown;
    onScreen?: () => boolean;
    commitBlocked?: () => Reason | null;
  } = {},
) {
  const mock = createMockApi();
  await mock.openPath(MOCK_FOLDER, "thisTab");
  const hash = await mock.pluginCall("review.open", { path: GUIDE });
  const guideHash = hash.ok ? hash.value.hash : "";
  const api: Api = {
    ...mock,
    pluginCall: async <M extends PluginMethod>(method: M, args: PluginMethods[M]["args"]) => {
      if (method === options.conflictOn) {
        const failure: Result<PluginMethods[M]["result"]> = {
          ok: false,
          error: {
            kind: "backend",
            command: "plugin_call",
            error: { kind: "conflict", subject: GUIDE, detail: "stale" },
          },
        };
        return failure;
      }
      const failOn = options.failOn;
      if (
        failOn !== undefined &&
        method === failOn.method &&
        (args as { basis?: string }).basis === failOn.basis
      ) {
        const failure: Result<PluginMethods[M]["result"]> = {
          ok: false,
          error: {
            kind: "backend",
            command: "plugin_call",
            error: { kind: "unavailable", subject: "noBasis", detail: null },
          },
        };
        return failure;
      }
      return mock.pluginCall(method, args);
    },
  };
  const shown = new Box<Shown>(options.shown ?? { path: GUIDE, hash: guideHash, kind: "blocks" });
  const notes: string[] = [];
  const notices: ReviewNotice[] = [];
  let repaints = 0;
  const store = new ReviewStore(
    new PluginClient(api, () => MOCK_FOLDER),
    () => MOCK_FOLDER,
    (text) => notes.push(text),
    {
      shown: () => shown.value,
      repaint: () => {
        repaints += 1;
      },
    },
    options.onScreen ?? (() => true),
    options.commitBlocked ?? (() => null),
    (notice) => {
      // the window's lasting notice: taking it back dismisses it (its onGone hears so)
      notices.push(notice);
      return () => {
        const at = notices.indexOf(notice);
        if (at < 0) return;
        notices.splice(at, 1);
        notice.onGone("dismissed");
      };
    },
  );
  const stop = store.start();
  await settle();
  return { mock, store, stop, shown, notes, notices, guideHash, repaints: () => repaints };
}

afterEach(() => vi.restoreAllMocks());

describe("the review store", () => {
  it("offers the changes of a document that has some, and nothing for one that has none", async () => {
    const { store, shown, stop } = await setup();
    expect(store.state).toMatchObject({ status: "offer", path: GUIDE, count: 2 });
    shown.value = { path: "README.md", hash: "ff".repeat(8), kind: "blocks" };
    await settle();
    expect(store.state.status).toBe("idle");
    stop();
  });

  it("shows the changes, keeps one, takes it back, reverts the rest and ends", async () => {
    const { store, notes, stop } = await setup();
    await store.begin();
    expect(store.state).toMatchObject({ status: "reviewing", at: 0 });
    expect(store.reviewing?.count).toBe(2);
    await store.keep();
    expect(store.reviewing?.count).toBe(1);
    expect(store.undoable).toBe(1);
    await store.undo();
    expect(store.reviewing?.count).toBe(2);
    expect(store.undoable).toBe(0);
    await store.revertAll();
    expect(store.state.status).toBe("idle");
    expect(notes.some((n) => n.length > 0)).toBe(true);
    stop();
  });

  it("moves between the changes, wrapping, and says where it is", async () => {
    const { store, stop } = await setup();
    await store.begin();
    store.go(1);
    expect(store.current).toBe(1);
    store.go(1);
    expect(store.current).toBe(0);
    store.go(-1);
    expect(store.current).toBe(1);
    expect(store.announcement).toMatch(/2/);
    stop();
  });

  it("steps to the next and previous change: an offer opens the review at the first or the last", async () => {
    const first = await setup();
    expect(first.store.state.status).toBe("offer");
    expect(await first.store.step(1)).toBe(true);
    expect(first.store.state).toMatchObject({ status: "reviewing", at: 0 });
    expect(await first.store.step(1)).toBe(true);
    expect(first.store.current).toBe(1);
    expect(await first.store.step(1)).toBe(true);
    expect(first.store.current).toBe(0);
    expect(await first.store.step(-1)).toBe(true);
    expect(first.store.current).toBe(1);
    first.stop();

    const last = await setup();
    expect(await last.store.step(-1)).toBe(true);
    expect(last.store.state).toMatchObject({ status: "reviewing", at: 1 });
    last.stop();
  });

  it("has no step for a document with nothing to review or one the offer is not for", async () => {
    const { store, shown, stop } = await setup();
    shown.value = { path: "README.md", hash: "ff".repeat(8), kind: "blocks" };
    await settle();
    expect(store.state.status).toBe("idle");
    expect(await store.step(1)).toBe(false);
    expect(await store.step(-1)).toBe(false);
    expect(store.state.status).toBe("idle");
    stop();

    const none = await setup({ shown: { path: null, hash: null, kind: null } });
    expect(await none.store.step(1)).toBe(false);
    none.stop();
  });

  it("repaints the page when the review starts, moves, and ends", async () => {
    const { store, repaints, stop } = await setup();
    const before = repaints();
    await store.begin();
    expect(repaints()).toBeGreaterThan(before);
    const mid = repaints();
    store.go(1);
    expect(repaints()).toBeGreaterThan(mid);
    const end = repaints();
    await store.done();
    expect(repaints()).toBeGreaterThan(end);
    expect(store.state.status).toBe("idle");
    stop();
  });

  it("lets go of a document when the person leaves it", async () => {
    const { mock, store, shown, stop } = await setup();
    await store.begin();
    shown.value = { path: "README.md", hash: "ee".repeat(8), kind: "blocks" };
    await settle();
    expect(mock.probe.plugins.calls).toContain("review.release");
    expect(store.state.status).toBe("idle");
    stop();
  });

  it("looks again when the file changed under the review, keeping the place", async () => {
    const { store, shown, stop } = await setup();
    await store.begin();
    store.go(1);
    shown.value = { path: GUIDE, hash: "dd".repeat(8), kind: "blocks" };
    await settle();
    expect(store.state).toMatchObject({ status: "reviewing", at: 1 });
    expect(store.announcement).not.toBe("");
    stop();
  });

  it("a stale answer refreshes the review and says so; nothing else changes", async () => {
    const { store, notes, stop } = await setup({ conflictOn: "review.revert" });
    await store.begin();
    await store.revert(0);
    await settle();
    expect(notes.at(0)).toBeDefined();
    expect(store.reviewing?.count).toBe(2);
    expect(store.undoable).toBe(0);
    stop();
  });

  it("will not review what is not a Markdown document, and says why when asked", async () => {
    const { store, shown, notes, stop } = await setup();
    shown.value = { path: "notes.txt", hash: "aa".repeat(8), kind: null };
    await settle();
    expect(store.state.status).toBe("idle");
    await store.begin();
    expect(store.reviewing).toBeNull();
    expect(notes).toHaveLength(0);
    stop();
  });

  it("the basis can be changed and the review is drawn again, and says so", async () => {
    const { store, mock, stop } = await setup();
    await store.begin();
    await store.setBasis("commit");
    expect(store.basis).toBe("commit");
    expect(mock.probe.plugins.calls).toContain("review.setBasis");
    expect(store.reviewing?.canKeep).toBe(false);
    expect(store.announcement).toMatch(
      /^Now comparing with the last commit\. \d+ changes? to review\.$/,
    );
    await store.setBasis("read");
    expect(store.announcement).toMatch(/^Now comparing with what you read\./);
    stop();
  });

  it('Done | Esc with changes left: "Review again" reopens at the same change; the changes are held till then', async () => {
    const { store, mock, notices, stop } = await setup();
    await store.begin();
    store.go(1);
    await store.done();
    expect(store.state.status).toBe("idle");
    expect(mock.probe.plugins.calls).not.toContain("review.release");
    const offered = notices[0]!;
    expect([offered.text, offered.actions.map((a) => a.label)]).toEqual([
      "Review closed.",
      ["Review again"],
    ]);
    // the window dismisses it as "action", then runs the button
    notices.splice(0, 1);
    offered.onGone("action");
    offered.actions[0]!.run();
    await settle();
    expect(store.state).toMatchObject({ status: "reviewing", at: 1 });
    expect(store.announcement).toMatch(/^Change 2 of 2/);
    expect(mock.probe.plugins.calls).not.toContain("review.release");
    stop();
  });

  it("the offer has no clock: it waits; dismissed or the document left, the changes are let go once", async () => {
    const { store, mock, notices, shown, stop } = await setup();
    const releases = (): number =>
      mock.probe.plugins.calls.filter((c) => c === "review.release").length;
    await store.begin();
    await store.done();
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(notices).toHaveLength(1);
    expect(releases()).toBe(0);
    // dismissed (x, Esc in it)
    const first = notices.splice(0, 1)[0]!;
    first.onGone("dismissed");
    await settle();
    expect(releases()).toBe(1);
    first.actions[0]!.run();
    await settle();
    expect(store.state.status).toBe("idle");
    // closed, then another document: the offer goes with the changes
    await store.begin();
    await store.done();
    expect(notices).toHaveLength(1);
    shown.value = { path: "README.md", hash: "ee".repeat(8), kind: "blocks" };
    await settle();
    expect(notices).toHaveLength(0);
    expect(releases()).toBe(2);
    stop();
  });

  it("asked for again another way (palette, key) while offered: the same change, and the offer goes", async () => {
    const { store, mock, notices, stop } = await setup();
    await store.begin();
    store.go(1);
    await store.done();
    await store.begin();
    expect(store.state).toMatchObject({ status: "reviewing", at: 1 });
    expect(notices).toHaveLength(0);
    expect(mock.probe.plugins.calls).not.toContain("review.release");
    stop();
  });

  it("a review closed with no changes left in it offers nothing", async () => {
    const { store, notices, stop } = await setup();
    await store.begin();
    await store.revertAll();
    await store.done();
    expect(notices).toEqual([]);
    stop();
  });

  it("without a repository the last commit is not chosen: the choice stays, a note says why", async () => {
    const noRepo: Reason = { need: "gitRepo", remedy: null };
    const { store, mock, notes, stop } = await setup({ commitBlocked: () => noRepo });
    expect(store.commitUnavailable).toBe("This folder isn't a git repository.");
    await store.begin();
    const before = store.reviewing;
    await store.setBasis("commit");
    expect(store.basis).toBe("read");
    expect(store.reviewing).toBe(before);
    expect(store.basisNote).toBe("This folder isn't a git repository.");
    expect(notes).toHaveLength(0);
    expect(mock.probe.plugins.calls).not.toContain("review.setBasis");
    // the note goes with the review
    await store.done();
    expect(store.basisNote).toBeNull();
    stop();
  });

  it("git missing says so (before the repository)", async () => {
    const { store, stop } = await setup({
      commitBlocked: () => ({ need: "gitAvailable", remedy: "settings.git" }),
    });
    expect(store.commitUnavailable).toBe("Git isn't available on this computer.");
    stop();
  });

  it("a basis the backend cannot draw keeps the review as it was and says so", async () => {
    const { store, mock, notes, stop } = await setup({
      failOn: { method: "review.open", basis: "commit" },
    });
    await store.begin();
    const before = store.reviewing;
    await store.setBasis("commit");
    expect(store.basis).toBe("read");
    expect(store.reviewing).toBe(before);
    expect(store.basisNote).toBe("This file can't be compared with that version.");
    expect(notes).toHaveLength(0);
    expect(mock.probe.plugins.calls).not.toContain("review.setBasis");
    stop();
  });

  it("rapid basis switches end on the last choice", async () => {
    const { store, stop } = await setup();
    await store.begin();
    await Promise.all([store.setBasis("commit"), store.setBasis("read"), store.setBasis("commit")]);
    expect(store.basis).toBe(store.reviewing?.basis);
    stop();
  });

  it("teardown leaves it idle", async () => {
    const { store, stop } = await setup();
    await store.begin();
    stop();
    expect(store.state.status).toBe("idle");
  });
});

const CODE = "src/main.rs";
const code: Shown = { path: CODE, hash: "0000000000000000", kind: "lines" };

describe("the review of code and plain text", () => {
  it("offers its changes and runs like the review of a page, by lines", async () => {
    const { store, stop } = await setup({ shown: code });
    expect(store.state).toMatchObject({ status: "offer", path: CODE, count: 2 });
    await store.begin();
    expect(store.reviewing?.unit).toBe("lines");
    expect(store.reviewing?.changes[0]?.old).toEqual(['    println!("hi");']);
    await store.revert();
    expect(store.reviewing?.count).toBe(1);
    await store.undo();
    expect(store.reviewing?.count).toBe(2);
    await store.keepAll();
    expect(store.state.status).toBe("idle");
    stop();
  });

  it("opens the review against the last commit when nothing is new since the person looked", async () => {
    const { store, notes, mock, stop } = await setup({ shown: code });
    // what was read is what is there
    await mock.pluginCall("review.keepAll", { path: CODE, hash: code.hash ?? "" });
    await store.begin();
    expect(notes.at(-1)).toMatch(/last commit/);
    expect(store.basis).toBe("commit");
    expect(store.reviewing).toMatchObject({
      unit: "lines",
      basis: "commit",
      canKeep: false,
      count: 1,
    });
    stop();
  });

  it("without a repository the review does not fall back to the last commit", async () => {
    const { store, notes, mock, stop } = await setup({
      shown: code,
      commitBlocked: () => ({ need: "gitRepo", remedy: null }),
    });
    await mock.pluginCall("review.keepAll", { path: CODE, hash: code.hash ?? "" });
    await store.begin();
    expect(store.reviewing).toBeNull();
    expect(store.basis).toBe("read");
    expect(notes.at(-1)).not.toMatch(/last commit/);
    stop();
  });

  it("tells the backend a code file was looked at after a moment in front of the person", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    const { mock, shown, stop } = await setup({ shown: code });
    await new Promise((resolve) => setTimeout(resolve, SEEN_MS + 200));
    expect(mock.probe.plugins.calls.filter((c) => c === "review.seen")).toHaveLength(1);
    // a Markdown page is followed by the unread plugin, and a file left before the moment is not looked at
    shown.value = { path: GUIDE, hash: "ab".repeat(8), kind: "blocks" };
    await new Promise((resolve) => setTimeout(resolve, SEEN_MS + 200));
    expect(mock.probe.plugins.calls.filter((c) => c === "review.seen")).toHaveLength(1);
    stop();
  });

  it("does not count a look while the window is not in front", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(false);
    const { mock, stop } = await setup({ shown: code });
    await new Promise((resolve) => setTimeout(resolve, SEEN_MS + 200));
    expect(mock.probe.plugins.calls).not.toContain("review.seen");
    stop();
  });

  it("does not count a look in a tab that is not on screen, even with the focus", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    const onScreen = new Box(false);
    const { mock, shown, stop } = await setup({ shown: code, onScreen: () => onScreen.value });
    await new Promise((resolve) => setTimeout(resolve, SEEN_MS + 200));
    // an agent rewrites the file in the hidden tab: the baseline does not move
    shown.value = { ...code, hash: "9".repeat(16) };
    await new Promise((resolve) => setTimeout(resolve, SEEN_MS + 200));
    expect(mock.probe.plugins.calls).not.toContain("review.seen");
    // the person comes to the tab
    onScreen.value = true;
    await new Promise((resolve) => setTimeout(resolve, SEEN_MS + 200));
    expect(mock.probe.plugins.calls.filter((c) => c === "review.seen")).toHaveLength(1);
    stop();
  });
});

describe("what is reviewed by lines", () => {
  const view = (body: DocumentView["body"], format = "text"): DocumentView => ({
    path: "x",
    name: "x",
    format,
    role: null,
    body,
    hash: "h",
    size: 1,
    mtimeMs: 1,
    editable: { editable: "surgical" },
    notes: [],
  });

  it("is text and config files, whole; a page by blocks; a file shown in parts or a picture not at all", () => {
    expect(kindOf(view({ kind: "text", text: "a", language: null }), "a.rs")).toBe("lines");
    expect(
      kindOf(
        view({ kind: "markdown", blocks: [], outline: [], frontMatter: null }, "markdown"),
        "README.md",
      ),
    ).toBe("blocks");
    expect(
      kindOf(
        view({ kind: "text", text: "a", language: null, partial: { loaded: 1, total: 9 } }),
        "big.log",
      ),
    ).toBeNull();
    expect(kindOf(view({ kind: "media", media: "image", mime: "image/png" }), "a.png")).toBeNull();
    expect(kindOf(view({ kind: "notice", notice: { notice: "binary" } }), "a.bin")).toBeNull();
  });
});
