import { afterEach, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import {
  MOCK_PROJECTS_BROKEN,
  MOCK_PROJECTS_FRESH,
  MOCK_PROJECTS_LISTED,
  MOCK_PROJECTS_MANY,
  MOCK_PROJECTS_ONE,
} from "../../api/mockProjects";
import { NOTICE_MS, ProjectsStore, type NoticeAction, type TimedNotice } from "./store.svelte";

type Fx = {
  api: ReturnType<typeof createMockApi>;
  store: ProjectsStore;
  /** Notices without buttons. */
  notes: string[];
  /** Notices with buttons. */
  actions: { text: string; actions: readonly NoticeAction[]; notice: TimedNotice }[];
  fails: string[];
  settings: string[];
  refocused: number[];
  /** The settings page shows (the host's answer). */
  shown: { value: boolean };
  stop: () => void;
};

let stops: (() => void)[] = [];
afterEach(() => {
  for (const stop of stops) stop();
  stops = [];
});

async function open(root: string): Promise<Fx> {
  const api = createMockApi();
  await api.openPath(root, "thisTab");
  const notes: string[] = [];
  const actions: Fx["actions"] = [];
  const fails: string[] = [];
  const settings: string[] = [];
  const refocused: number[] = [];
  const shown = { value: false };
  const store = new ProjectsStore(api, {
    notice: (notice) => {
      expect(notice.closeAfterMs).toBe(NOTICE_MS);
      if (notice.actions.length === 0) notes.push(notice.text);
      else actions.push({ text: notice.text, actions: notice.actions, notice });
    },
    fail: (text) => fails.push(text),
    openSettings: () => settings.push("open"),
    closeSettings: () => settings.push("close"),
    settingsShown: () => shown.value,
    refocus: () => refocused.push(1),
  });
  const stop = await store.start(root);
  stops.push(stop);
  return { api, store, notes, actions, fails, settings, refocused, shown, stop };
}

/** The words said to screen readers next (the store empties the region first, then sets them). */
async function said(fx: Fx): Promise<string> {
  await vi.waitFor(() => expect(fx.store.announcement).not.toBe(""));
  return fx.store.announcement;
}

const writes = (fx: Fx): string[] =>
  fx.api.probe.projects.calls.filter((c) => !c.startsWith("projects_view"));

describe("projects store", () => {
  it("a plain folder: nothing to ask, no projects, nothing written (A0)", async () => {
    const fx = await open(MOCK_FOLDER);
    expect(fx.store.present).toBe(false);
    expect(fx.store.strip.kind).toBe("none");
    expect(fx.store.members.size).toBe(0);
    expect(fx.store.askedThisOpen).toBe(false);
    expect(fx.store.ownerOf("docs/guide.md")).toBeNull();
    expect(writes(fx)).toEqual([]);
  });

  it("a folder holding one cloned repository asks once, naming it (A14)", async () => {
    const fx = await open(MOCK_PROJECTS_ONE);
    expect(fx.store.strip).toMatchObject({ kind: "first", count: 1, names: ["template-app"] });
  });

  it("Include all: projects included, a notice with Undo; Undo puts the question back (A2, A3)", async () => {
    const fx = await open(MOCK_PROJECTS_FRESH);
    expect(fx.store.strip).toMatchObject({ kind: "first", count: 4 });
    expect(fx.store.askedThisOpen).toBe(true);
    await fx.store.includeAll();
    expect(fx.store.strip.kind).toBe("none");
    expect(fx.store.members.size).toBe(4);
    expect(fx.store.memberAt("hubs/hub-b")?.state).toBe("included");
    expect(fx.store.ownerOf("hubs/hub-b/projects/spec/plan.md")?.path).toBe(
      "hubs/hub-b/projects/spec",
    );
    expect(fx.actions).toHaveLength(1);
    expect(fx.actions[0]?.text).toBe(
      "4 projects included. Change this any time in Settings > Projects inside.",
    );
    fx.actions[0]?.actions[0]?.run();
    await vi.waitFor(() => expect(fx.store.strip.kind).toBe("first"));
    expect(fx.store.members.size).toBe(0);
    // F3: the Undo button went with its notice: focus is handed on, the reader hears what happened
    await vi.waitFor(() => expect(fx.refocused).toHaveLength(1));
    expect(await said(fx)).toBe("Undone: 4 projects are no longer included.");
    // the code offer keeps waiting for the next open
    expect(fx.store.askedThisOpen).toBe(true);
  });

  it("No thanks on the first question: never asked again, projects listed as not included (A5)", async () => {
    const fx = await open(MOCK_PROJECTS_FRESH);
    await fx.store.declineOffered();
    // F9: where to change the mind
    expect(fx.notes).toEqual(["You can include projects later in Settings > Projects inside."]);
    expect(fx.store.view?.answer).toBe("declinedAll");
    expect(fx.store.strip.kind).toBe("none");
    expect(fx.store.present).toBe(true);
    expect(fx.store.view?.rows.every((r) => r.state === "declined")).toBe(true);
  });

  it("Choose…: all offered checked; unchecked ones are left out, checked ones included (A4)", async () => {
    const fx = await open(MOCK_PROJECTS_FRESH);
    fx.store.beginChoose();
    expect(fx.settings).toEqual(["open"]);
    expect([...(fx.store.choosing ?? [])].sort()).toEqual([
      "hubs/hub-b",
      "hubs/hub-b/projects/spec",
      "site",
      "tools/notes-app",
    ]);
    fx.store.setChosen("site", false);
    await fx.store.confirmChoose();
    expect(fx.settings).toEqual(["open", "close"]);
    expect(fx.store.choosing).toBeNull();
    expect(fx.store.members.size).toBe(3);
    expect(fx.store.memberAt("site")).toBeNull();
    expect(fx.store.view?.rows.find((r) => r.path === "site")?.state).toBe("declined");
    expect(fx.store.strip.kind).toBe("none");
  });

  it("Cancel saves nothing; the question stays", async () => {
    const fx = await open(MOCK_PROJECTS_FRESH);
    fx.store.beginChoose();
    fx.store.setChosen("site", false);
    fx.store.cancelChoose();
    expect(writes(fx)).toEqual([]);
    expect(fx.store.strip.kind).toBe("first");
  });

  it("a folder with a list: new project asked about, rules and agent kept, missing ones listed", async () => {
    const fx = await open(MOCK_PROJECTS_LISTED);
    expect(fx.store.strip).toMatchObject({
      kind: "new",
      count: 1,
      names: ["new-app"],
      paths: ["tools/new-app"],
    });
    const hub = fx.store.memberAt("hubs/hub-b");
    expect(hub?.name).toBe("PerMark software");
    await fx.store.setHidden(hub!, "own+root");
    expect(fx.store.memberAt("hubs/hub-b")?.hidden).toBe("own+root");
    await fx.store.setAgent(hub!, "off");
    expect(fx.store.memberAt("hubs/hub-b")?.agent).toBe("off");
    expect(await fx.store.setIncluded(fx.store.memberAt("tools/notes-app")!, false)).toBe(true);
    expect(fx.store.memberAt("tools/notes-app")).toBeNull();
    // F4: the screen reader hears where the row went
    expect(await said(fx)).toBe("notes-app moved to Not included.");
    const gone = fx.store.view?.rows.find((r) => r.state === "missing");
    expect(gone?.path).toBe("hubs/hub-x");
    // the mock's folder picker answers a folder outside: refused in words, nothing changed
    expect(await fx.store.locate(gone!, "Locate…")).toBe(
      "Choose a folder inside workspace-listed.",
    );
    // the picker opens at the old parent folder (R5: never the system's last folder)
    expect(fx.api.probe.pickedFrom).toEqual([`${MOCK_PROJECTS_LISTED}/hubs`]);
    fx.store.remove(gone!);
    expect(fx.store.view?.rows.some((r) => r.state === "missing")).toBe(false);
    expect(fx.fails).toEqual([]);
  });

  it("including from a row: the row moves (said), no notice", async () => {
    const fx = await open(MOCK_PROJECTS_LISTED);
    const vendor = fx.store.view!.rows.find((r) => r.path === "vendor-lib")!;
    expect(await fx.store.setIncluded(vendor, true)).toBe(true);
    expect(await said(fx)).toBe("vendor-lib moved to Included.");
    expect(fx.actions).toEqual([]);
    expect(fx.notes).toEqual([]);
  });

  it("Remove from list: the row goes at once; the entry leaves the list when the notice goes (F5)", async () => {
    const fx = await open(MOCK_PROJECTS_LISTED);
    const gone = fx.store.view!.rows.find((r) => r.state === "missing")!;
    fx.store.remove(gone);
    expect(fx.store.view?.rows.some((r) => r.uid === gone.uid)).toBe(false);
    expect(fx.actions.at(-1)?.text).toBe("hub-x removed from the list.");
    expect(writes(fx)).toEqual([]);
    // twice: one notice, one removal
    fx.store.remove(gone);
    expect(fx.actions).toHaveLength(1);
    fx.actions[0]!.notice.onGone?.("timeout");
    await vi.waitFor(() =>
      expect(writes(fx)).toEqual([`projects_remove ${JSON.stringify([gone.uid])}`]),
    );
    await vi.waitFor(() => expect(fx.store.removing.size).toBe(0));
    expect(fx.store.served?.rows.some((r) => r.uid === gone.uid)).toBe(false);
  });

  it("Undo of a removal: the entry never left (same id + rules); focus back on its row", async () => {
    const fx = await open(MOCK_PROJECTS_LISTED);
    const gone = fx.store.view!.rows.find((r) => r.state === "missing")!;
    fx.shown.value = true;
    fx.store.remove(gone);
    const notice = fx.actions[0]!;
    // the notice's button ran: it goes with how = "action" (no removal)
    notice.actions[0]!.run();
    notice.notice.onGone?.("action");
    expect(fx.store.view?.rows.find((r) => r.uid === gone.uid)).toEqual(gone);
    expect(fx.store.ask).toMatchObject({ kind: "row", key: gone.key });
    expect(await said(fx)).toBe("hub-x is back in the list.");
    await new Promise((r) => setTimeout(r, 20));
    expect(writes(fx)).toEqual([]);
  });

  it("a removal still waiting when the folder goes is dropped (the entry stays)", async () => {
    const fx = await open(MOCK_PROJECTS_LISTED);
    const gone = fx.store.view!.rows.find((r) => r.state === "missing")!;
    fx.store.remove(gone);
    fx.stop();
    fx.actions[0]!.notice.onGone?.("timeout");
    await new Promise((r) => setTimeout(r, 20));
    expect(writes(fx)).toEqual([]);
  });

  it("an unreadable list: read-only, the strip says so, a refused change is said in words", async () => {
    const fx = await open(MOCK_PROJECTS_BROKEN);
    expect(fx.store.readOnly).toBe(true);
    // F7: the Explorer says the list can't be read; Show the problem opens Settings at it
    expect(fx.store.strip.kind).toBe("broken");
    fx.store.showProblem();
    expect(fx.settings).toEqual(["open"]);
    expect(fx.store.ask).toMatchObject({ kind: "problem" });
    await fx.store.setIncluded(fx.store.view!.rows[0]!, true);
    expect(fx.fails).toHaveLength(1);
    expect(fx.store.members.size).toBe(0);
  });

  it("a large folder: 36 projects with long and Unicode names own their paths", async () => {
    const fx = await open(MOCK_PROJECTS_MANY);
    expect(fx.store.members.size).toBe(36);
    expect(fx.store.ownerOf("repos/数据分析项目/README.md")?.name).toBe("数据分析项目");
    // NFC spelling of a folder whose name is written NFD
    expect(fx.store.ownerOf("repos/Café NFD/README.md")).not.toBeNull();
    expect(fx.store.strip).toMatchObject({
      kind: "new",
      count: 3,
      paths: ["incoming/new-1", "incoming/new-2", "incoming/new-3"],
    });
  });

  // ---- adversarial ------------------------------------------------------------------------------------------------

  it("rapid include | undo: 25 rounds end where they began, one notice each", async () => {
    const fx = await open(MOCK_PROJECTS_FRESH);
    for (let n = 0; n < 25; n += 1) {
      await fx.store.includeAll();
      const undo = fx.actions.at(-1)?.actions[0];
      undo?.run();
      await vi.waitFor(() => expect(fx.store.members.size).toBe(0));
    }
    expect(fx.actions).toHaveLength(25);
    expect(fx.store.strip.kind).toBe("first");
    expect(fx.fails).toEqual([]);
  });

  it("an undo that comes too late changes nothing and says nothing", async () => {
    const fx = await open(MOCK_PROJECTS_FRESH);
    await fx.store.includeAll();
    const stale = fx.actions[0]!.actions[0]!;
    await fx.store.setIncluded(fx.store.memberAt("site")!, false);
    // a later include drops the older token
    await fx.store.setIncluded(
      fx.store.view!.rows.find((r) => r.path === "site")!,
      true,
    );
    stale.run();
    await new Promise((r) => setTimeout(r, 20));
    expect(fx.store.members.size).toBe(4);
    expect(fx.fails).toEqual([]);
  });

  it("follows changes from outside (debounced), ignores other folders and old news", async () => {
    const fx = await open(MOCK_PROJECTS_FRESH);
    const views = (): number =>
      fx.api.probe.projects.calls.filter((c) => c === "projects_view").length;
    const before = views();
    // another tab's folder: not ours
    fx.api.emit("projects-changed", { root: MOCK_PROJECTS_LISTED, seq: 99 });
    // old news of ours
    fx.api.emit("projects-changed", { root: MOCK_PROJECTS_FRESH, seq: 0 });
    await new Promise((r) => setTimeout(r, 150));
    expect(views()).toBe(before);
    // a burst: one read
    for (let n = 0; n < 5; n += 1) {
      fx.api.emit("projects-changed", { root: MOCK_PROJECTS_FRESH, seq: 50 + n });
    }
    await vi.waitFor(() => expect(views()).toBe(before + 1));
  });

  it("teardown: no read after the folder went, a new folder starts clean", async () => {
    const fx = await open(MOCK_PROJECTS_FRESH);
    fx.store.beginChoose();
    fx.stop();
    expect(fx.store.view).toBeNull();
    expect(fx.store.choosing).toBeNull();
    expect(fx.store.root).toBeNull();
    await fx.api.openPath(MOCK_PROJECTS_LISTED, "thisTab");
    stops.push(await fx.store.start(MOCK_PROJECTS_LISTED));
    expect(fx.store.view?.answer).toBe("answered");
    expect(fx.store.askedThisOpen).toBe(true);
  });
});
