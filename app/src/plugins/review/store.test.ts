// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  Api,
  GitSnapshot,
  PluginMethod,
  PluginMethods,
  Result,
  ReviewVersions,
  VersionGap,
  VersionRow,
  VersionsReason,
} from "../../api";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { MOCK_HEAD, MOCK_LONG_HISTORY } from "../../api/mockReviewVersions";
import { language } from "../../core/i18n";
import { PluginClient } from "../../core/plugins/client";
import { Box } from "../../core/testing/box.svelte";
import { exactTime } from "../../core/time";
import {
  ReviewStore,
  SEEN_MS,
  kindOf,
  problemText,
  type ReviewKind,
  type ReviewNotice,
} from "./store.svelte";
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
    unsaved?: () => boolean;
  } = {},
) {
  const mock = createMockApi();
  await mock.openPath(MOCK_FOLDER, "thisTab");
  const hash = await mock.pluginCall("review.open", { path: GUIDE });
  const guideHash = hash.ok ? hash.value.hash : "";
  // answers of a method held back until the test lets them go (a slow backend)
  const holds = new Map<PluginMethod, Promise<void>>();
  const hold = (method: PluginMethod): (() => void) => {
    let open = (): void => undefined;
    holds.set(
      method,
      new Promise<void>((resolve) => {
        open = resolve;
      }),
    );
    return () => {
      holds.delete(method);
      open();
    };
  };
  // answers a test makes up for a method (`undefined`: the mock answers)
  type Canned<M extends PluginMethod> = (
    args: PluginMethods[M]["args"],
  ) => PluginMethods[M]["result"] | undefined;
  const canned: Partial<{ [M in PluginMethod]: Canned<M> }> = {};
  // every call the store made, oldest first
  type Sent<M extends PluginMethod = PluginMethod> = { method: M; args: PluginMethods[M]["args"] };
  const sent: Sent[] = [];
  const argsOf = <M extends PluginMethod>(method: M): PluginMethods[M]["args"][] =>
    sent.filter((call): call is Sent<M> => call.method === method).map((call) => call.args);
  const api: Api = {
    ...mock,
    pluginCall: async <M extends PluginMethod>(method: M, args: PluginMethods[M]["args"]) => {
      sent.push({ method, args });
      const held = holds.get(method);
      if (held !== undefined) await held;
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
      const made = canned[method]?.(args);
      if (made !== undefined) {
        const success: Result<PluginMethods[M]["result"]> = { ok: true, value: made };
        return success;
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
    options.unsaved ?? (() => false),
  );
  const stop = store.start();
  await settle();
  return {
    mock,
    store,
    stop,
    shown,
    notes,
    notices,
    guideHash,
    hold,
    canned,
    argsOf,
    repaints: () => repaints,
  };
}

afterEach(() => vi.restoreAllMocks());

/** Presses the button of the lasting line on screen as the window does: the line goes, then its button runs. */
async function press(notices: ReviewNotice[]): Promise<void> {
  const offered = notices.splice(0, 1)[0]!;
  offered.onGone("action");
  offered.actions[0]!.run();
  await settle();
}

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
    const { store, notices, stop } = await setup();
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
    expect(notices.map((n) => n.text)).toEqual(["All changes reverted in guide.md"]);
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

  it("a review closed with no changes left in it offers no review again", async () => {
    const { store, notices, stop } = await setup();
    await store.begin();
    await store.revertAll();
    await store.done();
    expect(notices.flatMap((n) => n.actions.map((a) => a.label))).toEqual(["Undo"]);
    stop();
  });

  it("every change reverted: the review ends, and the line that says so offers Undo", async () => {
    const { store, notices, notes, argsOf, stop } = await setup();
    await store.begin();
    await store.revertAll();
    expect(store.state.status).toBe("idle");
    expect(store.undoable).toBe(0);
    expect(notes).toEqual([]);
    expect(notices.map((n) => [n.text, n.actions.map((a) => a.label)])).toEqual([
      ["All changes reverted in guide.md", ["Undo"]],
    ]);
    await press(notices);
    expect(argsOf("review.undo")).toEqual([{ path: GUIDE, basis: "read" }]);
    // the review is on screen again, with what was put back
    expect(store.state).toMatchObject({ status: "reviewing", at: 0 });
    expect(store.reviewing).toMatchObject({ basis: "read", count: 2 });
    expect(store.announcement).toBe("Undone. 2 changes.");
    expect(notices).toEqual([]);
    stop();
  });

  it("the last change reverted alone ends the review the same way: Undo takes it back, then the one before", async () => {
    const { store, notices, stop } = await setup();
    await store.begin();
    await store.revert(0);
    await store.revert(0);
    expect(store.state.status).toBe("idle");
    await press(notices);
    expect(store.reviewing?.count).toBe(1);
    // the revert before it is still the bar's to undo
    expect(store.undoable).toBe(1);
    await store.undo();
    expect(store.reviewing?.count).toBe(2);
    expect(store.undoable).toBe(0);
    stop();
  });

  it("every change kept: a plain line says so, with nothing to undo", async () => {
    const { store, notices, notes, stop } = await setup();
    await store.begin();
    await store.keepAll();
    expect(store.state.status).toBe("idle");
    expect(notes).toEqual(["All changes kept"]);
    expect(notices).toEqual([]);
    stop();
  });

  it("Undo of a review that ended: a file changed since says so; the document left or the plugin off take the offer", async () => {
    const stale = await setup({ conflictOn: "review.undo" });
    await stale.store.begin();
    await stale.store.revertAll();
    await press(stale.notices);
    expect(stale.notes).toEqual(["Can't undo: guide.md changed since."]);
    expect(stale.store.state.status).toBe("idle");
    stale.stop();

    const left = await setup();
    await left.store.begin();
    await left.store.revertAll();
    const offered = left.notices[0]!;
    left.shown.value = { path: "README.md", hash: "ee".repeat(8), kind: "blocks" };
    await settle();
    expect(left.notices).toEqual([]);
    // a press that was on its way does nothing on the other document
    offered.actions[0]!.run();
    await settle();
    expect(left.argsOf("review.undo")).toEqual([]);
    left.stop();

    const off = await setup();
    await off.store.begin();
    await off.store.revertAll();
    off.stop();
    expect(off.notices).toEqual([]);
  });

  it("the page shows the file as Undo wrote it before the answer comes: the review is drawn all the same", async () => {
    const { store, shown, notices, hold, stop } = await setup();
    await store.begin();
    await store.revertAll();
    const undone = hold("review.undo");
    const pressed = press(notices);
    shown.value = { path: GUIDE, hash: "dd".repeat(8), kind: "blocks" };
    await pressed;
    undone();
    await settle();
    expect(store.reviewing?.count).toBe(2);
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

  it("a review asked for on one document never opens on the next one shown before the answer", async () => {
    const { store, shown, mock, hold, stop } = await setup();
    const before = mock.probe.plugins.calls.length;
    const answer = hold("review.status");
    const asked = store.begin();
    shown.value = { path: "README.md", hash: "ee".repeat(8), kind: "blocks" };
    flushSync();
    answer();
    await asked;
    await settle();
    expect(store.reviewing).toBeNull();
    expect(store.state.status).toBe("idle");
    expect(mock.probe.plugins.calls.slice(before)).not.toContain("review.open");
    stop();
  });

  it("a keep or revert in flight when the person moves on changes nothing on the next document", async () => {
    const { store, shown, mock, hold, stop } = await setup();
    await store.begin();
    const announced = store.announcement;
    const reverted = hold("review.revert");
    const asked = store.revert(0);
    const status = hold("review.status");
    shown.value = { path: "README.md", hash: "ee".repeat(8), kind: "blocks" };
    flushSync();
    reverted();
    await asked;
    expect(store.undoable).toBe(0);
    expect(store.announcement).toBe(announced);
    // the review on screen is not of the document shown: its keys act on nothing
    const before = mock.probe.plugins.calls.length;
    await store.keep(0);
    await store.revertAll();
    await store.undo();
    expect(mock.probe.plugins.calls.slice(before)).toEqual([]);
    status();
    await settle();
    expect(store.state.status).toBe("idle");
    expect(store.undoable).toBe(0);
    stop();
  });

  it("two reverts sent before the first answer both count: Undo can take back each", async () => {
    const { store, stop } = await setup();
    await store.begin();
    // a held key repeats: the second goes out while the first is on its way
    await Promise.all([store.revert(0), store.revert(1)]);
    expect(store.undoable).toBe(2);
    expect(store.announcement).toBe("Reverted. 1 change left.");
    stop();
  });

  it("a revert answered after the page showed the new version of the file still counts", async () => {
    const { store, shown, hold, stop } = await setup();
    await store.begin();
    const reverted = hold("review.revert");
    const asked = store.revert(0);
    // the write reached the disk: the page shows the new version before the answer comes back
    shown.value = { path: GUIDE, hash: "dd".repeat(8), kind: "blocks" };
    await settle();
    reverted();
    await asked;
    expect(store.undoable).toBe(1);
    expect(store.reviewing?.count).toBe(1);
    expect(store.announcement).toBe("Reverted. 1 change left.");
    stop();
  });

  it("a review closed while a revert is on its way stays closed", async () => {
    const { store, hold, stop } = await setup();
    await store.begin();
    const reverted = hold("review.revert");
    const asked = store.revert(0);
    await store.done();
    reverted();
    await asked;
    expect(store.state.status).toBe("idle");
    expect(store.undoable).toBe(0);
    stop();
  });

  /** The calls after the last `review.open` the backend handled. */
  const afterOpen = (calls: string[]): string[] => calls.slice(calls.lastIndexOf("review.open"));

  it("a review that opens after the person left is let go (asked for, by Refresh, by a basis)", async () => {
    const leave = async (start: (f: Awaited<ReturnType<typeof setup>>) => Promise<void>) => {
      const f = await setup({ conflictOn: "review.revert" });
      await f.store.begin();
      const opened = f.hold("review.open");
      const asked = start(f);
      await settle();
      f.shown.value = { path: "README.md", hash: "ee".repeat(8), kind: "blocks" };
      await settle();
      opened();
      await asked;
      await settle();
      expect(afterOpen(f.mock.probe.plugins.calls)).toContain("review.release");
      expect(f.store.reviewing).toBeNull();
      f.stop();
    };
    // asked for again (a review open on another basis: begin opens it afresh)
    await leave(async (f) => {
      await f.store.done();
      f.notices.splice(0).forEach((n) => n.onGone("dismissed"));
      await f.store.begin();
    });
    // a stale keep | revert reads the review again
    await leave((f) => f.store.revert(0));
    // another basis
    await leave((f) => f.store.setBasis("commit"));
  });

  it("a basis chosen while the person leaves is not taken for the review it never drew", async () => {
    const { store, shown, hold, stop } = await setup();
    await store.begin();
    const setting = hold("review.setBasis");
    const asked = store.setBasis("commit");
    await settle();
    const status = hold("review.status");
    shown.value = { path: "README.md", hash: "ee".repeat(8), kind: "blocks" };
    flushSync();
    setting();
    await asked;
    expect(store.basis).toBe("read");
    status();
    await settle();
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

const README = "README.md";
const readme: Shown = { path: README, hash: "ee".repeat(8), kind: "blocks" };
const handbook: Shown = { path: MOCK_LONG_HISTORY, hash: "cc".repeat(8), kind: "blocks" };

/** The list of versions on screen. */
function listOf(store: ReviewStore) {
  const list = store.versions;
  if (list.status !== "open") throw new Error("the list of versions is closed");
  return list;
}

/** The repository as it is, at another commit. */
const movedTo = (now: GitSnapshot, oid: string): GitSnapshot => ({
  ...now,
  seq: now.seq + 1,
  head: { ...now.head, oid },
});

describe("the earlier versions of a document", () => {
  it("lists them, newest first, with what the history says about the file", async () => {
    const { store, hold, argsOf, stop } = await setup();
    expect(store.versions).toEqual({ status: "closed" });
    const answer = hold("review.versions");
    const asked = store.openVersions();
    await settle();
    expect(listOf(store)).toMatchObject({ path: GUIDE, loading: true, rows: [], problem: null });
    answer();
    await asked;
    const list = listOf(store);
    expect(list).toMatchObject({
      loading: false,
      next: null,
      renamedFrom: "docs/manual.md",
      cut: false,
      problem: null,
      note: null,
    });
    expect(list.rows).toHaveLength(4);
    expect(list.rows[0]?.commit).toBe(MOCK_HEAD);
    const times = list.rows.map((row) => row.atMs);
    expect(times).toEqual([...times].sort((a, b) => b - a));
    expect(argsOf("review.versions")).toEqual([{ path: GUIDE }]);
    stop();
  });

  const REASONS: [VersionsReason, string][] = [
    ["unavailable", "Earlier versions are not available here."],
    ["notCommitted", "This file is not saved in git yet."],
    ["onlyCurrent", "There is only one version: the one you see."],
    ["otherRepository", "This file belongs to another repository inside this folder."],
    ["slow", "Reading the history took too long."],
    ["failed", "The history could not be read."],
    ["unsupported", "This file is not text, so versions cannot be compared."],
  ];

  it.each(REASONS)("no list says why: %s", async (reason, line) => {
    const { store, canned, stop } = await setup();
    canned["review.versions"] = () => ({ state: "unavailable", reason });
    await store.openVersions();
    const list = listOf(store);
    expect(list).toMatchObject({ loading: false, rows: [], problem: { kind: reason } });
    expect(list.problem && problemText(list.problem)).toBe(line);
    stop();
  });

  it("unsaved text in the editor: asks to save first and reads nothing", async () => {
    const { store, argsOf, stop } = await setup({ unsaved: () => true });
    await store.openVersions();
    const { problem } = listOf(store);
    expect(problem).toEqual({ kind: "unsaved" });
    expect(problem && problemText(problem)).toBe(
      "Save your changes first to compare with earlier versions.",
    );
    expect(argsOf("review.versions")).toEqual([]);
    stop();
  });

  it("without git or a repository: says which, and reads nothing", async () => {
    const { store, argsOf, stop } = await setup({
      commitBlocked: () => ({ need: "gitRepo", remedy: null }),
      unsaved: () => true,
    });
    await store.openVersions();
    const { problem } = listOf(store);
    expect(problem).toEqual({ kind: "git", text: "This folder isn't a git repository." });
    expect(problem && problemText(problem)).toBe("This folder isn't a git repository.");
    expect(argsOf("review.versions")).toEqual([]);
    expect(argsOf("git.snapshot")).toEqual([]);
    stop();
  });

  it("a list the backend could not read is a problem; trying again reads it", async () => {
    const failing = await setup({ conflictOn: "review.versions" });
    await failing.store.openVersions();
    expect(listOf(failing.store)).toMatchObject({ loading: false, problem: { kind: "failed" } });
    failing.stop();

    const { store, canned, argsOf, stop } = await setup();
    let slow = true;
    canned["review.versions"] = () => (slow ? { state: "unavailable", reason: "slow" } : undefined);
    await store.openVersions();
    expect(listOf(store).problem).toEqual({ kind: "slow" });
    slow = false;
    await store.retryVersions();
    expect(listOf(store)).toMatchObject({ loading: false, problem: null });
    expect(listOf(store).rows).toHaveLength(4);
    expect(argsOf("review.versions")).toEqual([{ path: GUIDE }, { path: GUIDE }]);
    stop();
  });

  it("shows more: the next pages are added in order, to the end of the history", async () => {
    const { store, argsOf, stop } = await setup({ shown: handbook });
    await store.openVersions();
    const first = listOf(store);
    expect(first.rows).toHaveLength(50);
    expect(first.next).not.toBeNull();
    expect(first.cut).toBe(true);
    await store.moreVersions();
    const second = listOf(store);
    expect(second.rows).toHaveLength(100);
    expect(second.rows.slice(0, 50)).toEqual(first.rows);
    await store.moreVersions();
    const all = listOf(store);
    expect(all.rows).toHaveLength(120);
    expect(all.next).toBeNull();
    expect(new Set(all.rows.map((row) => row.commit)).size).toBe(120);
    const times = all.rows.map((row) => row.atMs);
    expect(times).toEqual([...times].sort((a, b) => b - a));
    // the end of the history: nothing more is asked
    await store.moreVersions();
    expect(argsOf("review.versions").map((args) => args.after)).toEqual([
      undefined,
      first.next,
      second.next,
    ]);
    stop();
  });

  /** A version as the history tells it. */
  const version = (commit: string, atMs: number, same = false): VersionRow => ({
    commit,
    atMs,
    author: "Ana Souza",
    coAuthors: [],
    subject: "Fix typos",
    same,
  });
  /** One page of a list. */
  const page = (
    rows: VersionRow[],
    next: number | null,
    renamedFrom: string | null = null,
  ): ReviewVersions => ({ state: "list", rows, next, renamedFrom, cut: false });
  const A = "a".repeat(40);
  const B = "b".repeat(40);
  const C = "c".repeat(40);

  it("a page with no rows that says there is more keeps the list askable", async () => {
    const { store, canned, argsOf, stop } = await setup();
    const pages = new Map<number | undefined, ReviewVersions>([
      [undefined, page([version(A, 3)], 7)],
      [7, page([], 9)],
      [9, page([version(B, 1)], null, "docs/old.md")],
    ]);
    canned["review.versions"] = ({ after }) => pages.get(after);
    await store.openVersions();
    await store.moreVersions();
    expect(listOf(store)).toMatchObject({ loading: false, next: 9, problem: null });
    expect(listOf(store).rows).toHaveLength(1);
    await store.moreVersions();
    expect(listOf(store)).toMatchObject({ next: null, renamedFrom: "docs/old.md" });
    expect(listOf(store).rows.map((r) => r.commit)).toEqual([A, B]);
    expect(argsOf("review.versions").map((args) => args.after)).toEqual([undefined, 7, 9]);
    stop();
  });

  it("a page that repeats a version already listed (the history moved between two pages) adds it once", async () => {
    const { store, canned, stop } = await setup();
    const pages = new Map<number | undefined, ReviewVersions>([
      [undefined, page([version(A, 3), version(B, 2)], 2)],
      [2, page([version(B, 2), version(C, 1)], null)],
    ]);
    canned["review.versions"] = ({ after }) => pages.get(after);
    await store.openVersions();
    await store.moreVersions();
    expect(listOf(store).rows.map((r) => r.commit)).toEqual([A, B, C]);
    stop();
  });

  it("pages with no version in them are read through until one has some: never a lone Show more", async () => {
    const { store, canned, argsOf, stop } = await setup();
    const pages = new Map<number | undefined, ReviewVersions>([
      [undefined, page([], 5)],
      [5, page([], 9)],
      [9, page([version(A, 1)], 12)],
    ]);
    canned["review.versions"] = ({ after }) => pages.get(after);
    await store.openVersions();
    expect(listOf(store)).toMatchObject({ loading: false, next: 12, problem: null });
    expect(listOf(store).rows.map((r) => r.commit)).toEqual([A]);
    expect(argsOf("review.versions").map((args) => args.after)).toEqual([undefined, 5, 9]);
    stop();
  });

  it("ten pages in a row with no version: it stops and says so, and Try again reads on from there", async () => {
    const { store, canned, argsOf, stop } = await setup();
    canned["review.versions"] = ({ after }) => page([], (after ?? 0) + 1);
    await store.openVersions();
    const stopped = listOf(store);
    expect(stopped).toMatchObject({
      loading: false,
      rows: [],
      next: 10,
      problem: { kind: "slow" },
    });
    expect(stopped.problem && problemText(stopped.problem)).toBe(
      "Reading the history took too long.",
    );
    expect(argsOf("review.versions")).toHaveLength(10);
    canned["review.versions"] = () => page([version(A, 1)], null);
    await store.retryVersions();
    expect(argsOf("review.versions").at(-1)).toEqual({ path: GUIDE, after: 10 });
    expect(listOf(store)).toMatchObject({ loading: false, next: null, problem: null });
    expect(listOf(store).rows.map((r) => r.commit)).toEqual([A]);
    stop();
  });

  it("the file changed under an open list: it is read again from its first page, with the mark of the same text", async () => {
    const { store, shown, canned, argsOf, stop } = await setup();
    let same = true;
    canned["review.versions"] = ({ after }) =>
      after === undefined ? page([version(A, 3, same)], 2) : page([version(B, 1)], null);
    await store.openVersions();
    await store.moreVersions();
    expect(listOf(store).rows.map((r) => r.same)).toEqual([true, false]);
    // the person | an agent wrote the file: the newest version no longer has its text
    same = false;
    shown.value = { path: GUIDE, hash: "dd".repeat(8), kind: "blocks" };
    await settle();
    expect(argsOf("review.versions").map((args) => args.after)).toEqual([undefined, 2, undefined]);
    expect(listOf(store)).toMatchObject({ loading: false, next: 2, problem: null, note: null });
    expect(listOf(store).rows.map((r) => [r.commit, r.same])).toEqual([[A, false]]);
    stop();
  });

  it("a page that comes back after the person left, or for a list closed meanwhile, is dropped", async () => {
    const { store, shown, hold, stop } = await setup({ shown: handbook });
    await store.openVersions();
    let answer = hold("review.versions");
    let asked = store.moreVersions();
    shown.value = readme;
    flushSync();
    expect(store.versions).toEqual({ status: "closed" });
    answer();
    await asked;
    await settle();
    expect(store.versions).toEqual({ status: "closed" });
    // closed and opened again while a page is on its way: the page of the list before is not added
    shown.value = handbook;
    await settle();
    await store.openVersions();
    answer = hold("review.versions");
    asked = store.moreVersions();
    store.closeVersions();
    const again = store.openVersions();
    answer();
    await Promise.all([asked, again]);
    expect(listOf(store).rows).toHaveLength(50);
    stop();
  });

  it("a picked version with changes: the review opens against it, the list closes, the count is said", async () => {
    const { store, argsOf, stop } = await setup();
    await store.openVersions();
    const row = listOf(store).rows[1]!;
    const version = { version: row.commit };
    await store.pickVersion(row);
    expect(store.versions).toEqual({ status: "closed" });
    expect(store.basis).toEqual(version);
    expect(store.picked).toEqual({ commit: row.commit, atMs: row.atMs });
    expect(store.reviewing).toMatchObject({
      path: GUIDE,
      basis: version,
      canKeep: false,
      uncommitted: true,
    });
    expect(store.state).toMatchObject({ status: "reviewing", at: 0 });
    expect(store.announcement).toMatch(
      /^Now comparing with the version of .*\d{4}.*\. \d+ changes? to review\.$/,
    );
    // a version is for this review only: it is never the choice the backend remembers
    expect(argsOf("review.setBasis")).toEqual([]);
    // each change is put back against the version, and Undo takes it back
    const count = store.reviewing?.count ?? 0;
    await store.revert(0);
    expect(argsOf("review.revert").at(-1)?.basis).toEqual(version);
    expect(store.reviewing).toMatchObject({ basis: version, count: count - 1 });
    await store.undo();
    expect(argsOf("review.undo").at(-1)?.basis).toEqual(version);
    expect(store.reviewing).toMatchObject({ basis: version, count });
    stop();
  });

  it("a version with the same text: a line says so, the list stays, nothing opens", async () => {
    const { store, argsOf, stop } = await setup({ shown: readme });
    await store.openVersions();
    const row = listOf(store).rows[0]!;
    expect(row.same).toBe(true);
    const before = argsOf("review.status").length;
    await store.pickVersion(row);
    expect(listOf(store).note).toBe("This version has the same text as the document now.");
    expect(store.basis).toBe("read");
    expect(store.picked).toBeNull();
    expect(store.reviewing).toBeNull();
    // the list said it already: the backend is not asked
    expect(argsOf("review.status")).toHaveLength(before);
    expect(argsOf("review.open")).toEqual([]);
    stop();
  });

  const GAPS: [VersionGap | "unknown", string][] = [
    ["missing", "This version is not on this computer."],
    ["tooLarge", "This version is too large to compare (over 2 MB)."],
    ["notText", "This version is not text, so it cannot be compared."],
    ["unknown", "This version cannot be compared."],
  ];

  it.each(GAPS)(
    "a version that cannot be compared (%s) says why in the list",
    async (gap, line) => {
      const { store, canned, argsOf, stop } = await setup();
      await store.openVersions();
      canned["review.status"] = ({ basis }) =>
        typeof basis !== "object"
          ? undefined
          : gap === "unknown"
            ? { state: { state: "noBasis" }, basis }
            : { state: { state: "noBasis" }, basis, gap };
      await store.pickVersion(listOf(store).rows[2]!);
      expect(listOf(store).note).toBe(line);
      expect(store.basis).toBe("read");
      expect(store.picked).toBeNull();
      expect(store.state.status).toBe("offer");
      expect(argsOf("review.open")).toEqual([]);
      stop();
    },
  );

  it("a document too large, not text, or a version that does not open: said in the list", async () => {
    const large = await setup();
    await large.store.openVersions();
    large.canned["review.status"] = ({ basis }) =>
      typeof basis === "object" ? { state: { state: "tooLarge" }, basis } : undefined;
    await large.store.pickVersion(listOf(large.store).rows[1]!);
    expect(listOf(large.store).note).toBe("This document is too large to compare (over 2 MB).");
    large.stop();

    const binary = await setup();
    await binary.store.openVersions();
    binary.canned["review.status"] = ({ basis }) =>
      typeof basis === "object" ? { state: { state: "unsupported" }, basis } : undefined;
    await binary.store.pickVersion(listOf(binary.store).rows[1]!);
    expect(listOf(binary.store).note).toBe(
      "This file is not text, so versions cannot be compared.",
    );
    binary.stop();

    const failing = await setup({ conflictOn: "review.open" });
    await failing.store.openVersions();
    await failing.store.pickVersion(listOf(failing.store).rows[1]!);
    expect(listOf(failing.store).note).toBe("This version cannot be compared.");
    expect(failing.store.basis).toBe("read");
    expect(failing.store.picked).toBeNull();
    failing.stop();
  });

  it("from an open review to a version and back to the last commit", async () => {
    const { store, argsOf, stop } = await setup();
    await store.begin();
    await store.revert(0);
    expect(store.undoable).toBe(1);
    await store.openVersions();
    const row = listOf(store).rows[2]!;
    await store.pickVersion(row);
    expect(store.reviewing?.basis).toEqual({ version: row.commit });
    expect(store.picked).toEqual({ commit: row.commit, atMs: row.atMs });
    // the same review goes on: what was reverted in it can still be undone
    expect(store.undoable).toBe(1);
    await store.setBasis("commit");
    expect(store.basis).toBe("commit");
    expect(store.picked).toBeNull();
    expect(store.reviewing?.basis).toBe("commit");
    expect(store.announcement).toMatch(/^Now comparing with the last commit\./);
    // only the choice is told to the backend, never a version
    expect(argsOf("review.setBasis")).toEqual([{ basis: "commit" }]);
    await store.setBasis({ version: row.commit });
    expect(store.basis).toBe("commit");
    expect(argsOf("review.setBasis")).toEqual([{ basis: "commit" }]);
    stop();
  });

  it("a picked version goes with its review: another document never opens against it", async () => {
    const { store, shown, argsOf, stop } = await setup();
    await store.openVersions();
    await store.pickVersion(listOf(store).rows[1]!);
    await store.openVersions();
    shown.value = code;
    await settle();
    expect(store.basis).toBe("read");
    expect(store.picked).toBeNull();
    expect(store.versions).toEqual({ status: "closed" });
    await store.begin();
    expect(store.reviewing).toMatchObject({ path: CODE, basis: "read" });
    const opened = argsOf("review.open").filter((args) => args.path === CODE);
    expect(opened.map((args) => args.basis)).toEqual([undefined]);
    stop();
  });

  it("the review closed, or everything brought back: the choice made before the pick is back", async () => {
    const closed = await setup();
    await closed.store.begin();
    await closed.store.setBasis("commit");
    await closed.store.openVersions();
    await closed.store.pickVersion(listOf(closed.store).rows[1]!);
    await closed.store.openVersions();
    await closed.store.done();
    expect(closed.store.basis).toBe("commit");
    expect(closed.store.picked).toBeNull();
    expect(closed.store.versions).toEqual({ status: "closed" });
    closed.stop();

    const reverted = await setup();
    await reverted.store.openVersions();
    const row = listOf(reverted.store).rows[1]!;
    await reverted.store.pickVersion(row);
    await reverted.store.revertAll();
    await reverted.store.confirmBringBack();
    expect(reverted.argsOf("review.revertAll").at(-1)?.basis).toEqual({ version: row.commit });
    expect(reverted.store.state.status).toBe("idle");
    expect(reverted.store.basis).toBe("read");
    expect(reverted.store.picked).toBeNull();
    reverted.stop();
  });

  it('"Review again" after a review against a version opens the same comparison', async () => {
    const { store, notices, stop } = await setup();
    await store.openVersions();
    const row = listOf(store).rows[1]!;
    await store.pickVersion(row);
    await store.done();
    expect(store.picked).toBeNull();
    const offered = notices.splice(0, 1)[0]!;
    offered.onGone("action");
    offered.actions[0]!.run();
    await settle();
    expect(store.reviewing?.basis).toEqual({ version: row.commit });
    expect(store.basis).toEqual({ version: row.commit });
    expect(store.picked).toEqual({ commit: row.commit, atMs: row.atMs });
    stop();
  });

  it("the repository at the same commit: an open list stays as it is", async () => {
    const { store, mock, argsOf, stop } = await setup();
    await store.openVersions();
    await settle();
    mock.probe.plugins.announceGit(mock.probe.plugins.git());
    await settle();
    expect(argsOf("review.versions")).toHaveLength(1);
    expect(listOf(store).note).toBeNull();
    stop();
  });

  it("the history moved: an open list is read again from its first page, and says so", async () => {
    const { store, mock, argsOf, stop } = await setup({ shown: handbook });
    await store.openVersions();
    await store.moreVersions();
    expect(listOf(store).rows).toHaveLength(100);
    mock.probe.plugins.announceGit(movedTo(mock.probe.plugins.git(), "f".repeat(40)));
    await settle();
    expect(listOf(store).rows).toHaveLength(50);
    expect(listOf(store).note).toBe("The history changed. The list was read again.");
    expect(argsOf("review.versions").at(-1)).toEqual({ path: MOCK_LONG_HISTORY });
    stop();
  });

  it("the history moved and the picked version is gone: back to the last commit, and a line says so", async () => {
    const { store, mock, canned, argsOf, stop } = await setup();
    await store.openVersions();
    const row = listOf(store).rows[1]!;
    await store.pickVersion(row);
    // the version is still there: the review stays on it
    mock.probe.plugins.announceGit(movedTo(mock.probe.plugins.git(), "e".repeat(40)));
    await settle();
    expect(store.picked?.commit).toBe(row.commit);
    expect(store.basisNote).toBeNull();
    canned["review.status"] = ({ basis }) =>
      typeof basis === "object"
        ? { state: { state: "noBasis" }, basis, gap: "missing" }
        : undefined;
    mock.probe.plugins.announceGit(movedTo(mock.probe.plugins.git(), "f".repeat(40)));
    await settle();
    expect(store.picked).toBeNull();
    expect(store.basis).toBe("commit");
    expect(store.reviewing?.basis).toBe("commit");
    expect(store.basisNote).toBe("The history changed. Comparing with the last commit again.");
    expect(argsOf("review.setBasis")).toEqual([{ basis: "commit" }]);
    stop();
  });

  it("the history moved and the picked version cannot be compared for another reason: the review stays on it", async () => {
    const { store, mock, canned, argsOf, stop } = await setup();
    await store.openVersions();
    const row = listOf(store).rows[1]!;
    await store.pickVersion(row);
    // no reason told (the file itself may be gone), or one that is not about the history
    const gaps: [VersionGap | undefined, string][] = [
      [undefined, "e"],
      ["notText", "f"],
    ];
    for (const [gap, oid] of gaps) {
      canned["review.status"] = ({ basis }) =>
        typeof basis !== "object"
          ? undefined
          : gap === undefined
            ? { state: { state: "noBasis" }, basis }
            : { state: { state: "noBasis" }, basis, gap };
      mock.probe.plugins.announceGit(movedTo(mock.probe.plugins.git(), oid.repeat(40)));
      await settle();
      expect(store.picked?.commit, oid).toBe(row.commit);
      expect(store.basis, oid).toEqual({ version: row.commit });
      expect(store.basisNote, oid).toBeNull();
    }
    expect(argsOf("review.setBasis")).toEqual([]);
    stop();
  });

  it("teardown closes the list, lets the version go and stops hearing the repository", async () => {
    const { store, mock, argsOf, stop } = await setup();
    await store.openVersions();
    await store.pickVersion(listOf(store).rows[1]!);
    await store.openVersions();
    stop();
    expect(store.versions).toEqual({ status: "closed" });
    expect(store.picked).toBeNull();
    expect(store.basis).toBe("read");
    const before = argsOf("review.status").length;
    mock.probe.plugins.announceGit(movedTo(mock.probe.plugins.git(), "f".repeat(40)));
    await settle();
    expect(argsOf("review.status")).toHaveLength(before);
  });
});

describe("bringing back an earlier version", () => {
  /** A review against the version in row `nth` of the list; the guide (the default) has text that is in no commit. */
  async function againstVersion(shown?: Shown, nth = 1) {
    const made = await setup(shown === undefined ? {} : { shown });
    await made.store.openVersions();
    const row = listOf(made.store).rows[nth]!;
    await made.store.pickVersion(row);
    return { ...made, row };
  }

  it("text that is in no commit would be replaced: it asks first, and only a yes writes", async () => {
    const { store, row, argsOf, stop } = await againstVersion();
    const view = store.reviewing;
    expect(view?.uncommitted).toBe(true);
    expect(store.confirming).toBe(false);
    await store.revertAll();
    expect(store.confirming).toBe(true);
    expect(argsOf("review.revertAll")).toEqual([]);
    // asked for again (the key pressed twice) is not a yes
    await store.revertAll();
    expect(argsOf("review.revertAll")).toEqual([]);
    // moving between the changes leaves the question up
    store.go(1);
    expect(store.confirming).toBe(true);
    await store.confirmBringBack();
    expect(argsOf("review.revertAll")).toEqual([
      { path: GUIDE, hash: view?.hash, basis: { version: row.commit } },
    ]);
    expect(store.confirming).toBe(false);
    expect(store.state.status).toBe("idle");
    stop();
  });

  it("brought back: the line names the version and offers Undo, which opens the same comparison again", async () => {
    const { store, row, notices, notes, argsOf, stop } = await againstVersion();
    const count = store.reviewing?.count;
    await store.revertAll();
    await store.confirmBringBack();
    expect(store.state.status).toBe("idle");
    expect(notes).toEqual([]);
    const when = exactTime(row.atMs, language.locale);
    expect(notices.map((n) => [n.text, n.actions.map((a) => a.label)])).toEqual([
      [`The version of ${when} was brought back.`, ["Undo"]],
    ]);
    await press(notices);
    expect(argsOf("review.undo")).toEqual([{ path: GUIDE, basis: { version: row.commit } }]);
    expect(store.reviewing).toMatchObject({ basis: { version: row.commit }, count });
    expect(store.basis).toEqual({ version: row.commit });
    expect(store.picked).toEqual({ commit: row.commit, atMs: row.atMs });
    stop();
  });

  it("a no takes the question back: nothing is written and the review stays", async () => {
    const { store, argsOf, stop } = await againstVersion();
    const count = store.reviewing?.count;
    await store.revertAll();
    store.cancelBringBack();
    expect(store.confirming).toBe(false);
    expect(store.reviewing?.count).toBe(count);
    // a yes with no question up writes nothing
    await store.confirmBringBack();
    expect(argsOf("review.revertAll")).toEqual([]);
    stop();
  });

  it("the question goes with what it was asked about: another document, another comparison", async () => {
    const left = await againstVersion();
    await left.store.revertAll();
    expect(left.store.confirming).toBe(true);
    left.shown.value = code;
    expect(left.store.confirming).toBe(false);
    await settle();
    expect(left.store.confirming).toBe(false);
    await left.store.confirmBringBack();
    expect(left.argsOf("review.revertAll")).toEqual([]);
    left.stop();

    const switched = await againstVersion();
    await switched.store.revertAll();
    await switched.store.setBasis("commit");
    expect(switched.store.confirming).toBe(false);
    await switched.store.confirmBringBack();
    expect(switched.argsOf("review.revertAll")).toEqual([]);
    // against the last commit there is nothing to ask
    await switched.store.revertAll();
    expect(switched.store.confirming).toBe(false);
    expect(switched.argsOf("review.revertAll").map((args) => args.basis)).toEqual(["commit"]);
    switched.stop();

    const repicked = await againstVersion();
    await repicked.store.revertAll();
    await repicked.store.openVersions();
    await repicked.store.pickVersion(listOf(repicked.store).rows[2]!);
    expect(repicked.store.confirming).toBe(false);
    repicked.stop();
  });

  it("the question goes when a change is put back alone, the review closes, or the plugin stops", async () => {
    const single = await againstVersion();
    await single.store.revertAll();
    await single.store.revert(0);
    expect(single.store.reviewing).not.toBeNull();
    expect(single.store.confirming).toBe(false);
    expect(single.argsOf("review.revertAll")).toEqual([]);
    single.stop();

    const closed = await againstVersion();
    await closed.store.revertAll();
    await closed.store.done();
    expect(closed.store.confirming).toBe(false);
    // "Review again" opens the comparison again, not the question
    const offered = closed.notices.splice(0, 1)[0]!;
    offered.onGone("action");
    offered.actions[0]!.run();
    await settle();
    expect(closed.store.picked).not.toBeNull();
    expect(closed.store.confirming).toBe(false);
    await closed.store.confirmBringBack();
    expect(closed.argsOf("review.revertAll")).toEqual([]);
    closed.stop();

    const stopped = await againstVersion();
    await stopped.store.revertAll();
    stopped.stop();
    expect(stopped.store.confirming).toBe(false);
    await stopped.store.confirmBringBack();
    expect(stopped.argsOf("review.revertAll")).toEqual([]);
  });

  it("a version of a file that is all in the last commit is brought back at once", async () => {
    const { store, row, argsOf, stop } = await againstVersion(readme, 2);
    expect(store.reviewing?.uncommitted).toBe(false);
    await store.revertAll();
    expect(store.confirming).toBe(false);
    expect(argsOf("review.revertAll").map((args) => args.basis)).toEqual([{ version: row.commit }]);
    expect(store.state.status).toBe("idle");
    stop();
  });
});
