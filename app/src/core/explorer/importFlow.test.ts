import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi } from "../../api/mock";
import type { Api, ApiError, ImportPlan, Result } from "../../api";
import { ImportFlow, SLOW_PLAN_MS, type ImportHost } from "./importFlow.svelte";

type Setup = ReturnType<typeof setup>;

function setup(api?: (mock: ReturnType<typeof createMockApi>) => Api) {
  const mock = createMockApi();
  const notes: string[] = [];
  const actions: { text: string; labels: string[]; run: (() => void)[] }[] = [];
  const failures: ApiError[] = [];
  const ends: (string | null)[] = [];
  const placed: string[][] = [];
  const counts = { undone: 0, focused: 0 };
  const undoneIds: number[] = [];
  const host: ImportHost = {
    fail: (e) => failures.push(e),
    note: (text) => notes.push(text),
    action: (text, ...list) =>
      actions.push({ text, labels: list.map((a) => a.label), run: list.map((a) => a.run) }),
    project: () => "demo",
    focus: () => (counts.focused += 1),
    end: (kept) => ends.push(kept),
    placed: (paths) => placed.push([...paths]),
    undo: (id) => {
      counts.undone += 1;
      undoneIds.push(id);
      return Promise.resolve();
    },
  };
  const flow = new ImportFlow(api ? api(mock) : mock, host);
  return { mock, flow, notes, actions, failures, ends, placed, counts, undoneIds };
}

let s: Setup;
let stop: () => void;
const AT = { x: 40, y: 50 };

beforeEach(async () => {
  s = setup();
  await s.mock.openPath("C:/Users/demo/permark-docs", "thisTab");
  stop = await s.flow.start();
});

afterEach(() => {
  stop();
  vi.useRealTimers();
});

async function dropped(paths: string[], folder = "docs"): Promise<void> {
  await s.flow.plan(folder, paths, AT);
}

function asked(): Extract<ImportFlow["state"], { status: "asking" }> {
  const state = s.flow.state;
  if (state.status !== "asking") throw new Error(`not asking: ${state.status}`);
  return state;
}

describe("the question", () => {
  it("asks at the drop point first: nothing is touched before the answer; Keep both preselected", async () => {
    await dropped(["C:/out/a.md", "C:/out/b.md"]);
    const state = asked();
    expect(state.plan.target).toBe("docs");
    expect(state.where).toBe("docs");
    expect(state.at).toEqual(AT);
    expect(state.conflicts).toBe("keepBoth");
    expect(s.counts.focused).toBe(1);
    expect(s.mock.probe.imports.applied).toEqual([]);
    expect(s.mock.probe.project.exists("docs/a.md")).toBe(false);
  });

  it("names the project folder by the project's name", async () => {
    await dropped(["C:/out/a.md"], "");
    expect(asked().where).toBe("demo");
  });

  it("lists the taken names before anything happens", async () => {
    // docs/guide.md exists in the demo project
    await dropped(["C:/out/guide.md", "C:/out/new.md"]);
    expect(asked().plan.items.map((i) => i.conflict)).toEqual([true, false]);
  });

  it("cancel leaves everything as it was and the tree closes what the drag opened", async () => {
    await dropped(["C:/out/a.md"]);
    s.flow.cancel();
    expect(s.flow.state.status).toBe("idle");
    expect(s.ends).toEqual([null]);
    expect(s.mock.probe.imports.applied).toEqual([]);
    // a second cancel (Esc, then the click outside) does nothing more
    s.flow.cancel();
    expect(s.ends).toEqual([null]);
  });

  it("a drop with nothing that can come in still asks: it says why, Copy does nothing", async () => {
    await dropped(["C:/out/.git"]);
    const state = asked();
    expect(state.plan.items).toEqual([]);
    expect(state.plan.refused.map((r) => r.reason)).toEqual(["protected"]);
    await s.flow.confirm("copy");
    expect(s.mock.probe.imports.applied).toEqual([]);
    expect(s.flow.state.status).toBe("asking");
  });

  it("a failed plan is an error notice and no question", async () => {
    const boom: ApiError = { kind: "command", command: "fileops_import_plan", message: "gone" };
    s = setup((mock) => ({
      ...mock,
      planImport: () => Promise.resolve({ ok: false, error: boom }),
    }));
    await s.flow.plan("docs", ["C:/out/a.md"], AT);
    expect(s.flow.state.status).toBe("idle");
    expect(s.failures).toEqual([boom]);
    expect(s.ends).toEqual([null]);
  });

  it("move is refused when something already lies in the project", async () => {
    await dropped(["C:/Users/demo/permark-docs/README.md"]);
    expect(asked().plan.items[0]?.inProject).toBe(true);
    await s.flow.confirm("move");
    expect(s.mock.probe.imports.applied).toEqual([]);
    await s.flow.confirm("copy");
    expect(s.mock.probe.imports.applied.at(-1)?.mode).toBe("copy");
  });
});

describe("a slow plan", () => {
  it("shows the question after 300 ms, checking names, then fills it in place", async () => {
    vi.useFakeTimers();
    let answer: (plan: Result<ImportPlan>) => void = () => {};
    s = setup((mock) => ({
      ...mock,
      planImport: () => new Promise<Result<ImportPlan>>((resolve) => (answer = resolve)),
    }));
    const planning = s.flow.plan("docs", ["C:/out/a.md"], AT);
    expect(s.flow.state).toMatchObject({ status: "planning", slow: false, where: "docs" });
    vi.advanceTimersByTime(SLOW_PLAN_MS - 1);
    expect(s.flow.state).toMatchObject({ slow: false });
    vi.advanceTimersByTime(1);
    expect(s.flow.state).toMatchObject({ status: "planning", slow: true, at: AT });
    expect(s.counts.focused).toBe(1);
    answer({ ok: true, value: { planId: 9, target: "docs", items: [], refused: [] } });
    await planning;
    expect(asked().at).toEqual(AT);
    expect(s.counts.focused).toBe(1);
  });

  it("a fast plan never shows the checking state", async () => {
    vi.useFakeTimers();
    await dropped(["C:/out/a.md"]);
    vi.advanceTimersByTime(SLOW_PLAN_MS * 2);
    expect(asked().plan.items).toHaveLength(1);
  });

  it("an answer that comes after a cancel is dropped", async () => {
    let answer: (plan: Result<ImportPlan>) => void = () => {};
    s = setup((mock) => ({
      ...mock,
      planImport: () => new Promise<Result<ImportPlan>>((resolve) => (answer = resolve)),
    }));
    const planning = s.flow.plan("docs", ["C:/out/a.md"], AT);
    s.flow.cancel();
    answer({ ok: true, value: { planId: 9, target: "docs", items: [], refused: [] } });
    await planning;
    expect(s.flow.state.status).toBe("idle");
  });

  it("a newer drop replaces the question; the older answer is dropped", async () => {
    const answers: ((plan: Result<ImportPlan>) => void)[] = [];
    s = setup((mock) => ({
      ...mock,
      planImport: () => new Promise<Result<ImportPlan>>((resolve) => answers.push(resolve)),
    }));
    const first = s.flow.plan("docs", ["C:/out/a.md"], AT);
    const second = s.flow.plan("notes", ["C:/out/b.md"], { x: 1, y: 2 });
    answers[1]?.({ ok: true, value: { planId: 2, target: "notes", items: [], refused: [] } });
    answers[0]?.({ ok: true, value: { planId: 1, target: "docs", items: [], refused: [] } });
    await Promise.all([first, second]);
    expect(asked().plan.planId).toBe(2);
    expect(s.ends).toEqual([]);
  });
});

describe("the import", () => {
  it("copies into the folder, selects what came in, keeps the folder open, offers undo", async () => {
    s.mock.probe.imports.outside.set("C:/out/pics", { kind: "folder", files: 3 });
    await dropped(["C:/out/a.md", "C:/out/pics"]);
    await s.flow.confirm("copy");
    expect(s.flow.state.status).toBe("idle");
    const project = s.mock.probe.project;
    expect(project.exists("docs/a.md")).toBe(true);
    expect(project.exists("docs/pics/file-3.txt")).toBe(true);
    expect(s.ends).toEqual(["docs"]);
    expect(s.placed).toEqual([["docs/a.md", "docs/pics"]]);
    expect(s.actions).toHaveLength(1);
    expect(s.actions[0]?.text).toBe("Copied 2 items to “docs”.");
    expect(s.actions[0]?.labels).toEqual(["Undo"]);
    s.actions[0]?.run[0]?.();
    expect(s.counts.undone).toBe(1);
  });

  it("the notice's Undo names this import, so a newer operation is not what it takes back", async () => {
    await dropped(["C:/out/a.md"]);
    await s.flow.confirm("copy");
    await dropped(["C:/out/b.md"]);
    await s.flow.confirm("copy");
    s.actions[0]?.run[0]?.();
    s.actions[1]?.run[0]?.();
    expect(s.undoneIds).toHaveLength(2);
    expect(s.undoneIds[0]).not.toBe(s.undoneIds[1]);
    // the mock takes back exactly the first one
    const first = s.undoneIds[0] ?? 0;
    expect((await s.mock.undoFileOpEntry(first)).ok).toBe(true);
    expect(s.mock.probe.project.exists("docs/a.md")).toBe(false);
    expect(s.mock.probe.project.exists("docs/b.md")).toBe(true);
    // asked again: gone
    const again = await s.mock.undoFileOpEntry(first);
    expect(again.ok && again.value).toBeNull();
  });

  it("an import that could not be journaled says what happened, with no Undo", async () => {
    s = setup((mock) => ({
      ...mock,
      applyImport: async (...args: Parameters<typeof mock.applyImport>) => {
        const result = await mock.applyImport(...args);
        return result.ok ? { ok: true, value: { ...result.value, undo: null } } : result;
      },
    }));
    await s.flow.plan("docs", ["C:/out/a.md"], AT);
    await s.flow.confirm("copy");
    expect(s.actions).toHaveLength(0);
    expect(s.notes.at(-1)).toBe("Copied 1 item to “docs”.");
  });

  it("moves when the person says Move here", async () => {
    await dropped(["C:/out/a.md"]);
    await s.flow.confirm("move");
    expect(s.mock.probe.imports.applied.at(-1)?.mode).toBe("move");
    expect(s.actions[0]?.text).toBe("Moved 1 item to “docs”.");
  });

  it("sends the choice about taken names; nothing added -> a note, folders close", async () => {
    await dropped(["C:/out/guide.md"]);
    s.flow.setConflicts("skip");
    await s.flow.confirm("copy");
    expect(s.mock.probe.imports.applied.at(-1)?.conflicts).toBe("skip");
    expect(s.notes.at(-1)).toContain("1 item was skipped because its name was taken.");
    expect(s.actions).toHaveLength(0);
    expect(s.ends).toEqual([null]);
  });

  it("keeps both: the new one is numbered and that name is selected", async () => {
    await dropped(["C:/out/guide.md"]);
    await s.flow.confirm("copy");
    expect(s.mock.probe.project.exists("docs/guide (2).md")).toBe(true);
    expect(s.placed).toEqual([["docs/guide (2).md"]]);
  });

  it("shows progress while it runs and ends idle", async () => {
    let release: () => void = () => {};
    s.mock.probe.imports.hold = new Promise<void>((resolve) => (release = resolve));
    await dropped(["C:/out/a.md", "C:/out/b.md"]);
    const running = s.flow.confirm("copy");
    expect(s.flow.busy).toBe(true);
    await vi.waitFor(() =>
      expect(s.flow.state.status === "running" && s.flow.state.progress).toBeTruthy(),
    );
    release();
    await running;
    expect(s.flow.state.status).toBe("idle");
    expect(s.flow.busy).toBe(false);
  });

  it("cancel stops it once; what was done stays", async () => {
    let release: () => void = () => {};
    s.mock.probe.imports.hold = new Promise<void>((resolve) => (release = resolve));
    await dropped(["C:/out/a.md", "C:/out/b.md"]);
    const running = s.flow.confirm("copy");
    await vi.waitFor(() => expect(s.flow.state.status).toBe("running"));
    await s.flow.stop();
    await s.flow.stop();
    release();
    await running;
    expect(s.mock.probe.imports.cancelled).toHaveLength(1);
    expect(s.notes.at(-1) ?? s.actions.at(-1)?.text).toContain("Stopped.");
  });

  it("a drop during an import is told to wait and does not disturb it", async () => {
    let release: () => void = () => {};
    s.mock.probe.imports.hold = new Promise<void>((resolve) => (release = resolve));
    await dropped(["C:/out/a.md"]);
    const running = s.flow.confirm("copy");
    await vi.waitFor(() => expect(s.flow.state.status).toBe("running"));
    await dropped(["C:/out/b.md"]);
    expect(s.notes).toEqual(["Wait for the current import to finish."]);
    // cancelling a question that is not there leaves the running import alone
    s.flow.cancel();
    s.flow.discard();
    expect(s.flow.state.status).toBe("running");
    release();
    await running;
    expect(s.mock.probe.project.exists("docs/a.md")).toBe(true);
    expect(s.mock.probe.project.exists("docs/b.md")).toBe(false);
  });

  it("a failing item is reported with the first reason; the others came in", async () => {
    s.mock.probe.imports.failing.add("bad.md");
    await dropped(["C:/out/ok.md", "C:/out/bad.md"]);
    await s.flow.confirm("copy");
    expect(s.actions[0]?.text).toContain("Copied 1 item to “docs”.");
    expect(s.actions[0]?.text).toContain("bad.md couldn't be added");
  });

  it("a refused apply is shown as an error", async () => {
    await dropped(["C:/out/a.md"]);
    // the plan is used up on the backend: the apply is refused
    await s.mock.applyImport(asked().plan.planId, "copy", "skip");
    await s.flow.confirm("copy");
    expect(s.failures).toHaveLength(1);
    expect(s.flow.state.status).toBe("idle");
    expect(s.ends).toEqual([null]);
  });

  it("reset forgets the question of the project that went away", async () => {
    await dropped(["C:/out/a.md"]);
    s.flow.reset();
    expect(s.flow.state.status).toBe("idle");
  });
});

describe("hostile input", () => {
  it("names with spaces, CJK and odd characters pass through unchanged", async () => {
    await dropped(["C:/out/文档 日本語 ü.md", "C:\\out\\back slash.md"]);
    expect(asked().plan.items.map((i) => i.name)).toEqual(["文档 日本語 ü.md", "back slash.md"]);
  });

  it("a thousand dropped entries make one question", async () => {
    const paths = Array.from({ length: 1000 }, (_, n) => `C:/out/file-${n}.md`);
    await dropped(paths);
    expect(asked().plan.items).toHaveLength(1000);
    expect(s.mock.probe.imports.planned).toHaveLength(1);
  });

  it("drops one after another, each answered, never overlap", async () => {
    for (let n = 0; n < 5; n++) {
      await dropped([`C:/out/r${n}.md`]);
      await s.flow.confirm("copy");
    }
    expect(s.flow.state.status).toBe("idle");
    for (let n = 0; n < 5; n++) expect(s.mock.probe.project.exists(`docs/r${n}.md`)).toBe(true);
  });
});
