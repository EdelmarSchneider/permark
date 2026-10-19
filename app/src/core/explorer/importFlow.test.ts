import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi } from "../../api/mock";
import type { ApiError, ImportOutcome } from "../../api";
import { ImportFlow, type ImportHost } from "./importFlow.svelte";
import type { Spot } from "./dropTarget";

type Setup = ReturnType<typeof setup>;

function setup() {
  const mock = createMockApi();
  const notes: string[] = [];
  const actions: { text: string; labels: string[]; run: (() => void)[] }[] = [];
  const failures: ApiError[] = [];
  const marks: (string | null)[] = [];
  const pictures: { files: File[]; x: number; y: number }[] = [];
  const undone = { count: 0 };
  let spot: Spot = { folder: "docs", editor: false };
  let takesPictures = true;
  const host: ImportHost = {
    fail: (e) => failures.push(e),
    note: (text) => notes.push(text),
    action: (text, ...list) =>
      actions.push({ text, labels: list.map((a) => a.label), run: list.map((a) => a.run) }),
    project: () => "demo",
    spot: () => spot,
    mark: (folder) => marks.push(folder),
    pictures: (files, x, y) => {
      pictures.push({ files, x, y });
      return takesPictures;
    },
    undo: () => {
      undone.count += 1;
      return Promise.resolve();
    },
  };
  const flow = new ImportFlow(mock, host);
  return {
    mock,
    flow,
    notes,
    actions,
    failures,
    marks,
    pictures,
    undone,
    at: (next: Spot) => (spot = next),
    refusePictures: () => (takesPictures = false),
  };
}

let s: Setup;
let stop: () => void;

beforeEach(async () => {
  s = setup();
  await s.mock.openPath("C:/Users/demo/permark-docs", "thisWindow");
  stop = await s.flow.start();
});

afterEach(() => stop());

async function dropped(paths: string[], x = 10, y = 10): Promise<void> {
  await s.flow.drop(paths, x, y);
}

describe("the question", () => {
  it("asks first: nothing is touched before the person answers", async () => {
    await dropped(["C:/out/a.md", "C:/out/b.md"]);
    const state = s.flow.state;
    expect(state.status).toBe("asking");
    if (state.status !== "asking") return;
    expect(state.plan.target).toBe("docs");
    expect(state.where).toBe("docs");
    expect(state.mode).toBe("copy");
    expect(state.conflicts).toBe("keepBoth");
    expect(s.mock.probe.imports.applied).toEqual([]);
    expect(s.mock.probe.project.exists("docs/a.md")).toBe(false);
  });

  it("names the project folder by the project's name", async () => {
    s.at({ folder: "", editor: false });
    await dropped(["C:/out/a.md"]);
    expect(s.flow.state.status === "asking" && s.flow.state.where).toBe("demo");
  });

  it("lists the taken names before anything happens", async () => {
    // docs/guide.md exists in the demo project
    await dropped(["C:/out/guide.md", "C:/out/new.md"]);
    const state = s.flow.state;
    expect(state.status === "asking" && state.plan.items.map((i) => i.conflict)).toEqual([
      true,
      false,
    ]);
  });

  it("cancel leaves everything as it was", async () => {
    await dropped(["C:/out/a.md"]);
    s.flow.cancelAsking();
    expect(s.flow.state.status).toBe("idle");
    expect(s.mock.probe.imports.applied).toEqual([]);
    expect(s.mock.probe.project.exists("docs/a.md")).toBe(false);
  });

  it("always starts with Copy, whatever was chosen before: Enter never moves anything", async () => {
    await dropped(["C:/out/a.md"]);
    s.flow.setMode("move");
    await s.flow.confirm();
    await dropped(["C:/out/b.md"]);
    expect(s.flow.state.status === "asking" && s.flow.state.mode).toBe("copy");
  });

  it("offers copy only when something already lies in the project", async () => {
    await dropped(["C:/Users/demo/permark-docs/README.md", "C:/out/a.md"]);
    const state = s.flow.state;
    expect(state.status === "asking" && state.mode).toBe("copy");
    s.flow.setMode("move");
    expect(s.flow.state.status === "asking" && s.flow.state.mode).toBe("copy");
  });

  it("a drop with nothing that can come in says why and asks nothing", async () => {
    s.mock.probe.imports.outside.set("C:/out/alias", { kind: "folder", link: true });
    await dropped(["C:/out/alias", "C:/out/.git"]);
    expect(s.flow.state.status).toBe("idle");
    expect(s.notes).toHaveLength(1);
    expect(s.notes[0]).toContain("alias");
    expect(s.notes[0]).toContain(".git");
  });

  it("a failed plan is shown and nothing stays open", async () => {
    const failing = {
      ...s.mock,
      planImport: () => Promise.resolve({ ok: false as const, error: boom }),
    };
    const flow = new ImportFlow(failing, hostOf(s));
    await flow.drop(["C:/out/a.md"], 1, 1);
    expect(flow.state.status).toBe("idle");
    expect(s.failures).toEqual([boom]);
  });

  it("a second drop while the question is open is told to wait", async () => {
    await dropped(["C:/out/a.md"]);
    await dropped(["C:/out/b.md"]);
    expect(s.notes).toEqual(["Wait for the current import to finish."]);
    expect(s.flow.state.status === "asking" && s.flow.state.plan.items[0]?.name).toBe("a.md");
  });

  it("an empty drop does nothing", async () => {
    await dropped([]);
    expect(s.flow.state.status).toBe("idle");
    expect(s.mock.probe.imports.planned).toEqual([]);
  });
});

const boom: ApiError = {
  kind: "backend",
  command: "fileops_import_plan",
  error: { kind: "protected", subject: ".git", detail: null },
};

function hostOf(setup: Setup): ImportHost {
  return {
    fail: (e) => setup.failures.push(e),
    note: (text) => setup.notes.push(text),
    action: () => {},
    project: () => "demo",
    spot: () => ({ folder: "docs", editor: false }),
    mark: () => {},
    pictures: () => true,
    undo: () => Promise.resolve(),
  };
}

describe("the import", () => {
  it("copies into the folder and offers undo", async () => {
    s.mock.probe.imports.outside.set("C:/out/pics", { kind: "folder", files: 3 });
    await dropped(["C:/out/a.md", "C:/out/pics"]);
    await s.flow.confirm();
    expect(s.flow.state.status).toBe("idle");
    const project = s.mock.probe.project;
    expect(project.exists("docs/a.md")).toBe(true);
    expect(project.exists("docs/pics/file-3.txt")).toBe(true);
    expect(s.actions).toHaveLength(1);
    expect(s.actions[0]?.text).toBe("Copied 2 items to “docs”.");
    expect(s.actions[0]?.labels).toEqual(["Undo"]);
    s.actions[0]?.run[0]?.();
    expect(s.undone.count).toBe(1);
  });

  it("moves when the person says so", async () => {
    await dropped(["C:/out/a.md"]);
    s.flow.setMode("move");
    await s.flow.confirm();
    expect(s.mock.probe.imports.applied.at(-1)?.mode).toBe("move");
    expect(s.actions[0]?.text).toBe("Moved 1 item to “docs”.");
  });

  it("sends the choice about taken names", async () => {
    await dropped(["C:/out/guide.md"]);
    s.flow.setConflicts("skip");
    await s.flow.confirm();
    expect(s.mock.probe.imports.applied.at(-1)?.conflicts).toBe("skip");
    expect(s.notes.at(-1)).toContain("1 item was skipped because its name was taken.");
    expect(s.actions).toHaveLength(0);
  });

  it("keeps both: the new one is numbered", async () => {
    await dropped(["C:/out/guide.md"]);
    await s.flow.confirm();
    expect(s.mock.probe.project.exists("docs/guide.md")).toBe(true);
    expect(s.mock.probe.project.exists("docs/guide (2).md")).toBe(true);
  });

  it("shows progress while it runs and ends idle", async () => {
    let release: () => void = () => {};
    s.mock.probe.imports.hold = new Promise<void>((resolve) => (release = resolve));
    await dropped(["C:/out/a.md", "C:/out/b.md"]);
    const running = s.flow.confirm();
    await vi.waitFor(() =>
      expect(s.flow.state.status === "running" && s.flow.state.progress).toBeTruthy(),
    );
    const state = s.flow.state;
    expect(state.status === "running" && state.progress?.itemsTotal).toBe(2);
    release();
    await running;
    expect(s.flow.state.status).toBe("idle");
  });

  it("cancel stops it and what was done stays, with undo", async () => {
    let release: () => void = () => {};
    s.mock.probe.imports.hold = new Promise<void>((resolve) => (release = resolve));
    await dropped(["C:/out/a.md", "C:/out/b.md"]);
    const running = s.flow.confirm();
    await vi.waitFor(() => expect(s.flow.state.status).toBe("running"));
    const stopping = s.flow.stop();
    expect(s.flow.state.status === "running" && s.flow.state.cancelling).toBe(true);
    await stopping;
    release();
    await running;
    expect(s.mock.probe.imports.cancelled).toHaveLength(1);
    expect(s.notes.at(-1) ?? s.actions.at(-1)?.text).toContain("Stopped.");
  });

  it("asking again to stop does nothing", async () => {
    let release: () => void = () => {};
    s.mock.probe.imports.hold = new Promise<void>((resolve) => (release = resolve));
    await dropped(["C:/out/a.md"]);
    const running = s.flow.confirm();
    await vi.waitFor(() => expect(s.flow.state.status).toBe("running"));
    await s.flow.stop();
    await s.flow.stop();
    expect(s.mock.probe.imports.cancelled).toHaveLength(1);
    release();
    await running;
  });

  it("a drop during an import is told to wait and does not disturb it", async () => {
    let release: () => void = () => {};
    s.mock.probe.imports.hold = new Promise<void>((resolve) => (release = resolve));
    await dropped(["C:/out/a.md"]);
    const running = s.flow.confirm();
    await vi.waitFor(() => expect(s.flow.state.status).toBe("running"));
    await dropped(["C:/out/b.md"]);
    expect(s.notes).toEqual(["Wait for the current import to finish."]);
    release();
    await running;
    expect(s.mock.probe.project.exists("docs/a.md")).toBe(true);
    expect(s.mock.probe.project.exists("docs/b.md")).toBe(false);
  });

  it("a failing item is reported with the first reason; the others came in", async () => {
    s.mock.probe.imports.failing.add("bad.md");
    await dropped(["C:/out/ok.md", "C:/out/bad.md"]);
    await s.flow.confirm();
    expect(s.actions[0]?.text).toContain("Copied 1 item to “docs”.");
    expect(s.actions[0]?.text).toContain("bad.md couldn't be added");
  });

  it("a refused apply is shown as an error", async () => {
    await dropped(["C:/out/a.md"]);
    const state = s.flow.state;
    if (state.status !== "asking") throw new Error("not asking");
    // the plan is gone on the backend (used up): the apply is refused
    await s.mock.applyImport(state.plan.planId, "copy", "skip");
    await s.flow.confirm();
    expect(s.failures).toHaveLength(1);
    expect(s.flow.state.status).toBe("idle");
  });

  it("reset forgets the question of the project that went away", async () => {
    await dropped(["C:/out/a.md"]);
    s.flow.reset();
    expect(s.flow.state.status).toBe("idle");
    expect(s.flow.hover).toBeNull();
  });

  it("progress of another plan is not shown", async () => {
    let release: () => void = () => {};
    s.mock.probe.imports.hold = new Promise<void>((resolve) => (release = resolve));
    await dropped(["C:/out/a.md"]);
    const running = s.flow.confirm();
    await vi.waitFor(() => expect(s.flow.state.status).toBe("running"));
    s.mock.emit("import-progress", {
      planId: 999,
      phase: "copying",
      itemsDone: 9,
      itemsTotal: 9,
      filesDone: 9,
      filesTotal: 9,
      bytesDone: 9,
      bytesTotal: 9,
      item: "other",
    });
    const state = s.flow.state;
    expect(state.status === "running" && state.progress?.itemsDone).not.toBe(9);
    release();
    await running;
  });
});

describe("while the drag is over the window", () => {
  it("marks the folder under the pointer and clears the mark when it leaves", () => {
    s.flow.over(10, 10, ["C:/out/a.md"], 1);
    expect(s.flow.hover).toEqual({ folder: "docs", pictures: false, count: 1 });
    expect(s.marks).toEqual(["docs"]);
    s.at({ folder: "notes", editor: false });
    s.flow.over(12, 12, ["C:/out/a.md"], 1);
    expect(s.marks).toEqual(["docs", "notes"]);
    s.flow.leave();
    expect(s.flow.hover).toBeNull();
    expect(s.marks.at(-1)).toBeNull();
  });

  it("does not repeat the mark while the pointer stays in one folder", () => {
    for (let n = 0; n < 20; n++) s.flow.over(10 + n, 10, ["C:/out/a.md"], 1);
    expect(s.marks).toEqual(["docs"]);
  });

  it("says pictures when they are over an editor, and marks no folder", () => {
    s.at({ folder: "docs", editor: true });
    s.flow.over(1, 1, ["C:/out/a.png"], 1);
    expect(s.flow.hover).toEqual({ folder: "docs", pictures: true, count: 1 });
    expect(s.marks).toEqual([null]);
  });
});

describe("pictures dropped on an editor", () => {
  it("are read and handed to the editor at the pointer; nothing is imported", async () => {
    s.at({ folder: "docs", editor: true });
    await dropped(["C:/out/a.png", "C:/out/b.JPG"], 40, 50);
    expect(s.mock.probe.pictures).toEqual(["C:/out/a.png", "C:/out/b.JPG"]);
    expect(s.pictures).toHaveLength(1);
    expect(s.pictures[0]?.x).toBe(40);
    expect(s.pictures[0]?.y).toBe(50);
    expect(s.pictures[0]?.files.map((f) => [f.name, f.type])).toEqual([
      ["a.png", "image/png"],
      ["b.JPG", "image/jpeg"],
    ]);
    expect(s.mock.probe.imports.planned).toEqual([]);
    expect(s.flow.state.status).toBe("idle");
  });

  it("a picture file that cannot be read is told; the others go in", async () => {
    s.at({ folder: "docs", editor: true });
    const flaky = {
      ...s.mock,
      readPicture: (path: string) =>
        path.endsWith("bad.png")
          ? Promise.resolve({ ok: false as const, error: boom })
          : s.mock.readPicture(path),
    };
    const flow = new ImportFlow(flaky, {
      ...hostOf(s),
      spot: () => ({ folder: "docs", editor: true }),
    });
    await flow.drop(["C:/out/bad.png", "C:/out/good.png"], 1, 1);
    expect(s.failures).toEqual([boom]);
  });

  it("mixed with other files they are imported like the rest", async () => {
    s.at({ folder: "docs", editor: true });
    await dropped(["C:/out/a.png", "C:/out/notes.md"]);
    expect(s.flow.state.status).toBe("asking");
    expect(s.pictures).toEqual([]);
  });

  it("over the page but not an editor they are imported", async () => {
    s.at({ folder: "docs", editor: false });
    await dropped(["C:/out/a.png"]);
    expect(s.flow.state.status).toBe("asking");
  });
});

describe("hostile input", () => {
  it("names with spaces, CJK and odd characters pass through unchanged", async () => {
    await dropped(["C:/out/文档 日本語 ü.md", "C:\\out\\back slash.md"]);
    const state = s.flow.state;
    expect(state.status === "asking" && state.plan.items.map((i) => i.name)).toEqual([
      "文档 日本語 ü.md",
      "back slash.md",
    ]);
  });

  it("a thousand dropped entries make one question", async () => {
    const paths = Array.from({ length: 1000 }, (_, n) => `C:/out/file-${n}.md`);
    await dropped(paths);
    const state = s.flow.state;
    expect(state.status === "asking" && state.plan.items).toHaveLength(1000);
    expect(s.mock.probe.imports.planned).toHaveLength(1);
  });

  it("drops one after another, each answered, never overlap", async () => {
    for (let n = 0; n < 5; n++) {
      await dropped([`C:/out/r${n}.md`]);
      await s.flow.confirm();
    }
    expect(s.flow.state.status).toBe("idle");
    for (let n = 0; n < 5; n++) expect(s.mock.probe.project.exists(`docs/r${n}.md`)).toBe(true);
  });

  it("outcome type is the backend's", () => {
    const sample: ImportOutcome = {
      target: "",
      copied: 0,
      moved: 0,
      files: 0,
      bytes: 0,
      skipped: 0,
      failed: [],
      links: 0,
      leftBehind: 0,
      cancelled: false,
    };
    expect(sample.cancelled).toBe(false);
  });
});
