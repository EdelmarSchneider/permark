// A tab switch while a drop question is open: the question waits in its tab (the view is hidden, its state kept) and
// is answered when the person comes back. Real ImportFlow + DropFlow on the mock backend.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi } from "../../api/mock";
import type { Api, ImportPlan, Result } from "../../api";
import { ImportFlow, SLOW_PLAN_MS } from "../explorer/importFlow.svelte";
import { DropFlow, type Frames } from "./dropFlow.svelte";

type Setup = Awaited<ReturnType<typeof setup>>;

async function setup(patch: (mock: ReturnType<typeof createMockApi>) => Partial<Api> = () => ({})) {
  const mock = createMockApi();
  await mock.openPath("C:/Users/demo/permark-docs", "thisTab");
  const api: Api = { ...mock, ...patch(mock) };
  const tab = { shown: true };
  const notes: string[] = [];
  const tree: string[] = [];
  const counts = { focused: 0, applied: 0 };
  const imports = new ImportFlow(api, {
    fail: (e) => notes.push(`fail ${e.kind}`),
    note: (text) => notes.push(text),
    action: (text) => notes.push(text),
    project: () => "demo",
    view: () => "main",
    focus: () => (counts.focused += 1),
    shown: () => tab.shown,
    end: (kept) => tree.push(`end ${kept}`),
    placed: () => undefined,
    undo: () => Promise.resolve(),
  });
  const frames: Frames = () => () => undefined;
  const drop = new DropFlow(
    api,
    {
      workspace: () => "project",
      spot: () => ({ area: "explorer", folder: "docs" }),
      folderName: (folder) => folder,
      tree: {
        over: (folder) => tree.push(`over ${folder}`),
        hold: () => tree.push("hold"),
        end: (kept) => tree.push(`end ${kept}`),
      },
      imports,
      pictures: () => true,
      fail: (e) => notes.push(`fail ${e.kind}`),
      note: (text) => notes.push(text),
      shown: () => tab.shown,
    },
    frames,
  );
  const stop = await imports.start();
  return {
    mock,
    imports,
    drop,
    notes,
    tree,
    counts,
    stop,
    /** The person switches to another tab and back (`App`: the effect calls `hidden()` when the tab leaves). */
    away: () => {
      tab.shown = false;
      drop.hidden();
    },
    back: () => (tab.shown = true),
  };
}

let s: Setup;

beforeEach(async () => {
  s = await setup();
});

afterEach(() => {
  s.stop();
  vi.useRealTimers();
});

describe("a tab switch while the import question is open", () => {
  it("the question waits in its tab and copies when answered after coming back", async () => {
    await s.drop.drop(["C:/out/a.md"], 40, 50);
    expect(s.imports.state.status).toBe("asking");
    s.tree.length = 0;
    s.away();
    // only the drag mark goes: the question and the folders it is about stay
    expect(s.imports.state.status).toBe("asking");
    expect(s.tree).toEqual(["over null"]);
    s.back();
    expect(s.imports.state.status).toBe("asking");
    await s.imports.confirm("copy");
    expect(s.mock.probe.project.exists("docs/a.md")).toBe(true);
    expect(s.notes).toEqual(["Copied 1 item to “docs”."]);
  });

  it("Move here still works after the switch", async () => {
    await s.drop.drop(["C:/out/a.md"], 40, 50);
    s.away();
    s.back();
    await s.imports.confirm("move");
    expect(s.mock.probe.imports.applied.at(-1)?.mode).toBe("move");
  });

  it("rapid switches (20 away | back) keep one question; one answer applies once", async () => {
    await s.drop.drop(["C:/out/a.md", "C:/out/b.md"], 40, 50);
    const plan = s.imports.state.status === "asking" ? s.imports.state.plan.planId : -1;
    for (let n = 0; n < 20; n++) {
      s.away();
      s.back();
    }
    const state = s.imports.state;
    expect(state.status === "asking" && state.plan.planId).toBe(plan);
    await s.imports.confirm("copy");
    await s.imports.confirm("copy");
    expect(s.mock.probe.imports.applied).toHaveLength(1);
  });

  it("Cancel after coming back still closes it, nothing copied, the tree's folders close", async () => {
    await s.drop.drop(["C:/out/a.md"], 40, 50);
    s.away();
    s.back();
    s.tree.length = 0;
    s.imports.cancel();
    expect(s.imports.state.status).toBe("idle");
    expect(s.tree).toEqual(["end null"]);
    expect(s.mock.probe.imports.applied).toEqual([]);
  });

  it("a plan that arrives while the tab is away becomes the question there, without taking the keyboard", async () => {
    let release: () => void = () => undefined;
    s.stop();
    s = await setup((mock) => ({
      planImport: (folder, sources) =>
        new Promise<Result<ImportPlan>>((done) => {
          release = () => void mock.planImport(folder, sources).then(done);
        }),
    }));
    const dropped = s.drop.drop(["C:/out/a.md"], 40, 50);
    expect(s.imports.state.status).toBe("planning");
    s.away();
    release();
    await dropped;
    expect(s.imports.state.status).toBe("asking");
    expect(s.counts.focused).toBe(0);
    s.back();
    await s.imports.confirm("copy");
    expect(s.mock.probe.project.exists("docs/a.md")).toBe(true);
  });

  it("the slow-plan timer is released when the project leaves the view meanwhile", async () => {
    vi.useFakeTimers();
    s.stop();
    s = await setup(() => ({ planImport: () => new Promise(() => undefined) }));
    void s.drop.drop(["C:/out/a.md"], 40, 50);
    s.away();
    expect(vi.getTimerCount()).toBe(1);
    s.imports.reset();
    expect(vi.getTimerCount()).toBe(0);
    vi.advanceTimersByTime(SLOW_PLAN_MS * 2);
    expect(s.imports.state.status).toBe("idle");
  });
});

describe("the question cannot be kept (the project left the view)", () => {
  it("a notice says the drop was cancelled; nothing was added", async () => {
    await s.drop.drop(["C:/out/a.md"], 40, 50);
    s.away();
    s.imports.reset();
    expect(s.imports.state.status).toBe("idle");
    expect(s.notes).toEqual(["The drop onto “docs” was cancelled. Nothing was added."]);
    expect(s.mock.probe.imports.applied).toEqual([]);
  });

  it("no notice when no question was open", () => {
    s.imports.reset();
    expect(s.notes).toEqual([]);
  });
});

describe("a tab switch while the many-tabs question is open", () => {
  it("waits and opens them when answered after coming back", async () => {
    s.stop();
    s = await setup();
    const paths = Array.from({ length: 9 }, (_, n) => `C:/many/${n}.md`);
    // route Open: the pointer is outside the Explorer list
    const flow = new DropFlow(s.mock, {
      workspace: () => "project",
      spot: () => ({ area: "other" }),
      folderName: (f) => f,
      tree: { over: () => undefined, hold: () => undefined, end: () => undefined },
      imports: s.imports,
      pictures: () => true,
      fail: () => undefined,
      note: () => undefined,
      shown: () => true,
    });
    await flow.drop(paths, 1, 1);
    flow.hidden();
    expect(flow.ask.status).toBe("asking");
    await flow.confirmOpen();
    expect(s.mock.probe.drops.opened).toEqual([{ paths, scope: "tabs" }]);
  });
});
