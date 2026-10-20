// @vitest-environment jsdom
import "../../core/testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { describe, expect, it } from "vitest";
import type { HandOffPreview, HandOffSessionView } from "../../api";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { PluginClient } from "../../core/plugins/client";
import { approx, canConfirm, included, notes, toggled, type Ready } from "./handoff";
import HandOffDialog from "./HandOffDialog.svelte";
import { HandOffStore, handOff } from "./handoff.svelte";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 5; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

function session(id: number, extra: Partial<HandOffSessionView> = {}): HandOffSessionView {
  return {
    id,
    start: 1_760_000_000_000 + id * 3_600_000,
    end: 1_760_000_000_000 + id * 3_600_000 + 1_800_000,
    actions: 3,
    rows: 2,
    tokens: 120,
    stale: false,
    checked: true,
    dropped: false,
    ...extra,
  };
}

function ready(extra: Partial<Ready> = {}): Ready {
  return {
    state: "ready",
    case: "several",
    first: false,
    sessions: [session(1), session(2)],
    outside: { files: 0, byFolder: false, git: null },
    allStale: false,
    trimmed: false,
    upgraded: false,
    rows: 4,
    tokens: 240,
    upto: 9,
    ...extra,
  };
}

describe("hand-off arithmetic", () => {
  it("takes the checked sessions that fit and flips one at a time", () => {
    const preview = ready({
      sessions: [session(1, { checked: false }), session(2), session(3, { dropped: true })],
    });
    expect(included(preview).map((s) => s.id)).toEqual([2]);
    expect(toggled(preview, 1)).toEqual([1, 2, 3]);
    expect(toggled(preview, 2)).toEqual([3]);
  });

  it("needs a session or a change outside to hand off", () => {
    const none = ready({ sessions: [session(1, { checked: false })] });
    expect(canConfirm(none)).toBe(false);
    expect(canConfirm({ ...none, outside: { files: 3, byFolder: false, git: null } })).toBe(true);
  });

  it("notes the case: outside only, first with older work, all stale, trimmed, upgraded", () => {
    expect(notes(ready({ case: "outsideOnly", sessions: [] }))).toEqual([
      "agent.handoff.outsideOnly",
    ]);
    expect(notes(ready({ case: "first" }))).toEqual(["agent.handoff.first"]);
    expect(notes(ready({ case: "first", sessions: [session(1)] }))).toEqual([]);
    expect(notes(ready({ case: "stale", allStale: true, trimmed: true, upgraded: true }))).toEqual([
      "agent.handoff.allStale",
      "agent.handoff.trimmed",
      "agent.handoff.upgraded",
    ]);
    expect(notes(ready({ case: "one" }))).toEqual([]);
  });

  it("reads sizes short", () => {
    expect(approx(340, "en")).toBe("340");
    expect(approx(1240, "en")).toBe("1.2k");
    expect(approx(12_400, "en")).toBe("12k");
    expect(approx(1240, "de")).toBe("1,2k");
  });
});

async function setup(connected = true) {
  const api = createMockApi();
  await api.openPath(MOCK_FOLDER, "thisTab");
  const copied: string[] = [];
  const said: string[] = [];
  const store = new HandOffStore(
    new PluginClient(api, () => MOCK_FOLDER),
    (text) => {
      copied.push(text);
      return Promise.resolve();
    },
    (text) => said.push(text),
    () => connected,
  );
  return { api, store, copied, said };
}

describe("hand-off flow", () => {
  it("previews, re-asks on every check, confirms, copies the line and says how many actions went", async () => {
    const { api, store, copied, said } = await setup();
    await store.open();
    expect(store.state.status).toBe("open");
    if (store.state.status !== "open") return;
    expect(store.state.full).toBe(false);
    expect(store.state.preview.sessions.map((s) => [s.id, s.checked, s.stale])).toEqual([
      [1, false, true],
      [2, true, false],
    ]);
    // the stale session goes in: the preview is asked again with the choice
    await store.toggle(1);
    if (store.state.status !== "open") return;
    expect(store.state.preview.sessions.every((s) => s.checked)).toBe(true);
    await store.confirm();
    expect(store.state.status).toBe("closed");
    expect(api.probe.plugins.agent.handedOff).toEqual([{ upto: 42, checked: [1, 2] }]);
    expect(copied).toEqual([
      "PerMark: 30 actions since last hand-off -> call activity since=handoff",
    ]);
    expect(said).toEqual(["Handed off 30 actions — copied, ready to paste in your prompt."]);
    // nothing new since: no dialog, a quiet note
    await store.open();
    expect(store.state.status).toBe("closed");
    expect(said.at(-1)).toBe("Nothing new since the last hand-off.");
  });

  it("copies the full list for agents without the connection, and cancel changes nothing", async () => {
    const { api, store, copied } = await setup(false);
    await store.open();
    if (store.state.status !== "open") throw new Error("open");
    expect(store.state.full).toBe(true);
    store.close();
    expect(api.probe.plugins.agent.handedOff).toEqual([]);
    expect(copied).toEqual([]);
    await store.open();
    await store.confirm();
    expect(copied[0]?.startsWith("activity since=handoff")).toBe(true);
  });

  it("draws the sessions, the outside summary and the total; the full-list choice is a checkbox", async () => {
    const { store } = await setup();
    // the dialog reads the shared store
    handOff.state = store.state;
    await store.open();
    handOff.state = store.state;
    const target = document.createElement("div");
    document.body.append(target);
    const view = mount(HandOffDialog, { target });
    await settle();
    const text = target.textContent ?? "";
    expect(target.querySelector('[role="dialog"]')).not.toBeNull();
    expect(text).toContain("over a week old");
    expect(text).toContain("14 files changed outside PerMark");
    expect(text).toContain("HEAD a1b2c3d..e4f5a6b (3 commits)");
    expect(text).toContain("Total: 26 actions");
    const boxes = target.querySelectorAll<HTMLInputElement>('input[type="checkbox"]');
    expect([...boxes].map((b) => b.checked)).toEqual([false, true, false]);
    // Escape closes without handing off
    target
      .querySelector('[role="dialog"]')
      ?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await settle();
    expect(handOff.state.status).toBe("closed");
    void unmount(view);
    target.remove();
  });
});

describe("the preview type", () => {
  it("is nothing | ready", () => {
    const nothing: HandOffPreview = { state: "nothing" };
    expect(nothing.state).toBe("nothing");
  });
});
