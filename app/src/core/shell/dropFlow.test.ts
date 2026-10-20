import { beforeEach, describe, expect, it } from "vitest";
import { createMockApi } from "../../api/mock";
import type { Api, ApiError, Result } from "../../api";
import type { DropContext, DropSpot } from "../explorer/importModel";
import { DropFlow, type DropHost, type Frames, type Point } from "./dropFlow.svelte";

type Setup = ReturnType<typeof setup>;

/** A view: what it shows, what is under the pointer, a tree, an import question, an editor. Frames run on `flush`. */
function setup(patch: (mock: ReturnType<typeof createMockApi>) => Partial<Api> = () => ({})) {
  const mock = createMockApi();
  let workspace: DropContext["workspace"] = "project";
  let spot: DropSpot = { area: "other" };
  const log: string[] = [];
  const notes: string[] = [];
  const failures: ApiError[] = [];
  const pictures: { names: string[]; at: Point }[] = [];
  const imports = {
    busy: false,
    planned: [] as { folder: string; paths: string[]; at: Point }[],
    plan(folder: string, paths: readonly string[], at: Point): Promise<void> {
      this.planned.push({ folder, paths: [...paths], at });
      return Promise.resolve();
    },
    cancel: () => log.push("imports.cancel"),
    discard: () => log.push("imports.discard"),
  };
  const queued: (() => void)[] = [];
  const frames: Frames = (run) => {
    queued.push(run);
    return () => {
      const at = queued.indexOf(run);
      if (at >= 0) queued.splice(at, 1);
    };
  };
  const host: DropHost = {
    workspace: () => workspace,
    spot: () => spot,
    folderName: (folder) => (folder === "" ? "demo" : (folder.split("/").at(-1) ?? folder)),
    tree: {
      over: (folder, point) => log.push(`tree.over ${folder} ${point ? "point" : "-"}`),
      hold: () => log.push("tree.hold"),
      end: (kept) => log.push(`tree.end ${kept}`),
    },
    imports,
    pictures: (files, at) => {
      pictures.push({ names: files.map((f) => f.name), at });
      return true;
    },
    fail: (e) => failures.push(e),
    note: (text) => notes.push(text),
  };
  const api: Api = { ...mock, ...patch(mock) };
  const flow = new DropFlow(api, host, frames);
  return {
    mock,
    flow,
    log,
    notes,
    failures,
    pictures,
    imports,
    queued,
    flush: () => {
      for (const run of queued.splice(0)) run();
    },
    show: (next: DropContext["workspace"]) => (workspace = next),
    under: (next: DropSpot) => (spot = next),
  };
}

let s: Setup;

beforeEach(() => {
  s = setup();
});

const tabs = async (): Promise<string[]> => {
  const listed = await s.mock.listWindows();
  return listed.ok ? (listed.value[0]?.tabs.map((t) => t.path) ?? []) : [];
};

describe("while dragging", () => {
  it("the badge says what a drop here does, near the pointer", () => {
    s.under({ area: "explorer", folder: "docs" });
    s.flow.over(30, 40, ["C:/a.md", "C:/b.md", "C:/c"]);
    expect(s.flow.hover).toBeNull();
    s.flush();
    expect(s.flow.hover?.badge).toEqual({ icon: "import", text: "Add 3 items to “docs”" });
    expect(s.flow.hover?.at).toEqual({ x: 30, y: 40 });
    expect(s.log).toEqual(["tree.over docs point"]);
    s.under({ area: "other" });
    s.flow.over(31, 41, ["C:/a.md", "C:/b.md", "C:/c"]);
    s.flush();
    expect(s.flow.hover?.badge).toEqual({ icon: "open", text: "Open 3 items in new tabs" });
    expect(s.log.at(-1)).toBe("tree.over null -");
  });

  it("an over storm draws once per frame, the newest point", () => {
    s.under({ area: "explorer", folder: "docs" });
    for (let n = 0; n < 1000; n++) s.flow.over(n, n, ["C:/a.md"]);
    expect(s.queued).toHaveLength(1);
    s.flush();
    expect(s.flow.hover?.at).toEqual({ x: 999, y: 999 });
    expect(s.log).toEqual(["tree.over docs point"]);
  });

  it("leave clears the badge, the pending frame and the tree's mark (folders it opened close)", () => {
    s.flow.over(1, 1, ["C:/a.md"]);
    s.flow.leave();
    expect(s.queued).toHaveLength(0);
    expect(s.flow.hover).toBeNull();
    expect(s.log).toEqual(["tree.end null"]);
  });

  it("over the Explorer while an import runs: wait, and no mark", () => {
    s.imports.busy = true;
    s.under({ area: "explorer", folder: "docs" });
    s.flow.over(1, 1, ["C:/a.md"]);
    s.flush();
    expect(s.flow.hover?.badge.text).toBe("Wait for the current import to finish.");
    expect(s.log).toEqual(["tree.over null point"]);
  });

  it("the Notes tab: loose files say Notes, folders open as tabs", () => {
    s.show("notes");
    s.flow.notes = { takes: (p) => p.endsWith(".md"), open: () => Promise.resolve() };
    s.flow.over(1, 1, ["C:/a.md"]);
    s.flush();
    expect(s.flow.hover?.badge).toEqual({ icon: "notes", text: "Open in Notes" });
    s.flow.over(1, 1, ["C:/a.md", "C:/dir"]);
    s.flush();
    expect(s.flow.hover?.badge.text).toBe("Open 2 items");
  });
});

describe("the drop", () => {
  it("on the Explorer: the tree holds its folders, the import question is asked at the point", async () => {
    s.under({ area: "explorer", folder: "docs" });
    s.flow.over(5, 5, ["C:/a.md"]);
    await s.flow.drop(["C:/a.md"], 7, 8);
    expect(s.queued).toHaveLength(0);
    expect(s.flow.hover).toBeNull();
    expect(s.imports.planned).toEqual([{ folder: "docs", paths: ["C:/a.md"], at: { x: 7, y: 8 } }]);
    expect(s.log).toEqual(["imports.discard", "tree.hold"]);
  });

  it("on the Explorer while an import runs: told to wait, nothing queued", async () => {
    s.imports.busy = true;
    s.under({ area: "explorer", folder: "docs" });
    await s.flow.drop(["C:/a.md"], 1, 1);
    expect(s.notes).toEqual(["Wait for the current import to finish."]);
    expect(s.imports.planned).toEqual([]);
    expect(s.log).toEqual(["imports.discard", "tree.end null"]);
  });

  it("pictures over an editor: read and inserted at the point; the tree's mark ends", async () => {
    s.under({ area: "editor", pictures: true });
    await s.flow.drop(["C:/out/a.png", "C:/out/b.JPG"], 40, 50);
    expect(s.mock.probe.pictures).toEqual(["C:/out/a.png", "C:/out/b.JPG"]);
    expect(s.pictures).toEqual([{ names: ["a.png", "b.JPG"], at: { x: 40, y: 50 } }]);
    expect(s.log).toContain("tree.end null");
    expect(s.mock.probe.drops.opened).toEqual([]);
  });

  it("a picture that cannot be read is told; the others go in", async () => {
    const boom: ApiError = { kind: "command", command: "picture_read", message: "x" };
    s = setup((mock) => ({
      readPicture: (path: string) =>
        path.endsWith("bad.png")
          ? Promise.resolve({ ok: false as const, error: boom })
          : mock.readPicture(path),
    }));
    s.under({ area: "editor", pictures: true });
    await s.flow.drop(["C:/out/bad.png", "C:/out/good.png"], 1, 1);
    expect(s.failures).toEqual([boom]);
    expect(s.pictures[0]?.names).toEqual(["good.png"]);
  });

  it("anywhere else: each path opens as a tab, the last one shown", async () => {
    s.show("welcome");
    await s.flow.drop(["C:/out/a.md", "C:/out/文档 x", "C:/out/b c.md"], 1, 1);
    expect(s.mock.probe.drops.opened).toEqual([
      { paths: ["C:/out/a.md", "C:/out/文档 x", "C:/out/b c.md"], scope: "tabs" },
    ]);
    expect(await tabs()).toEqual(["C:/out/a.md", "C:/out/文档 x", "C:/out/b c.md"]);
    expect(s.notes).toEqual([]);
  });

  it("a path already open: no second tab (one place)", async () => {
    s.show("project");
    await s.mock.openPath("C:/out/a.md", "tab");
    await s.flow.drop(["C:/out/a.md", "C:/out/b.md"], 1, 1);
    expect(await tabs()).toEqual(["C:/out/a.md", "C:/out/b.md"]);
  });

  it("failures make one notice for the drop, the rest opens", async () => {
    s.show("welcome");
    s.mock.probe.drops.missing.add("C:/gone/a.zip");
    s.mock.probe.drops.missing.add("C:/gone/b");
    await s.flow.drop(["C:/gone/a.zip", "C:/ok.md", "C:/gone/b"], 1, 1);
    expect(s.notes).toHaveLength(1);
    expect(s.notes[0]).toMatch(/^2 items couldn't be opened\. The first was “a\.zip”: /);
    expect(await tabs()).toEqual(["C:/ok.md"]);
  });

  it("more than 8 paths ask first, at the drop point; Open them opens all, Cancel nothing", async () => {
    s.show("project");
    const paths = Array.from({ length: 9 }, (_, n) => `C:/many/${n}.md`);
    await s.flow.drop(paths, 12, 34);
    expect(s.flow.ask).toEqual({ status: "asking", paths, at: { x: 12, y: 34 } });
    expect(s.mock.probe.drops.focused).toBe(1);
    expect(s.mock.probe.drops.opened).toEqual([]);
    s.flow.cancelOpen();
    expect(s.flow.ask.status).toBe("none");
    await s.flow.drop(paths, 1, 1);
    await s.flow.confirmOpen();
    expect(s.flow.ask.status).toBe("none");
    expect((await tabs()).length).toBe(9);
    // exactly 8: no question
    s = setup();
    await s.flow.drop(paths.slice(0, 8), 1, 1);
    expect(s.flow.ask.status).toBe("none");
    expect(s.mock.probe.drops.opened).toHaveLength(1);
  });

  it("two hundred paths: one question, then one call", async () => {
    const paths = Array.from({ length: 200 }, (_, n) => `C:/m/${n} é.md`);
    await s.flow.drop(paths, 1, 1);
    await s.flow.confirmOpen();
    expect(s.mock.probe.drops.opened).toHaveLength(1);
    expect((await tabs()).length).toBe(200);
  });

  it("a new drop closes an open question, whatever it lands on", async () => {
    const paths = Array.from({ length: 9 }, (_, n) => `C:/many/${n}.md`);
    await s.flow.drop(paths, 1, 1);
    s.under({ area: "explorer", folder: "" });
    await s.flow.drop(["C:/a.md"], 2, 2);
    expect(s.flow.ask.status).toBe("none");
    expect(s.imports.planned).toHaveLength(1);
    expect(s.log.filter((l) => l === "imports.discard")).toHaveLength(2);
  });

  it("an empty drop does nothing but end the drag", async () => {
    await s.flow.drop([], 1, 1);
    expect(s.mock.probe.drops.opened).toEqual([]);
    expect(s.log).toEqual(["imports.discard", "tree.end null"]);
  });

  it("an open that the backend refuses is one error notice", async () => {
    const boom: ApiError = { kind: "command", command: "open_dropped", message: "x" };
    s = setup(() => ({
      openDropped: (): Promise<Result<never>> => Promise.resolve({ ok: false, error: boom }),
    }));
    await s.flow.drop(["C:/a.md"], 1, 1);
    expect(s.failures).toEqual([boom]);
  });
});

describe("the Notes tab", () => {
  it("loose files: open elsewhere come forward, the rest into the Notes; folders open as tabs", async () => {
    s.show("notes");
    await s.mock.openPath("C:/open.md", "tab");
    const loose: string[][] = [];
    s.flow.notes = {
      takes: (p) => p.endsWith(".md"),
      open: (paths) => {
        loose.push(paths);
        return Promise.resolve();
      },
    };
    await s.flow.drop(["C:/new.md", "C:/open.md", "C:/folder"], 1, 1);
    expect(s.mock.probe.drops.opened).toEqual([
      { paths: ["C:/new.md", "C:/open.md"], scope: "existingOnly" },
      { paths: ["C:/folder"], scope: "tabs" },
    ]);
    expect(loose).toEqual([["C:/new.md"]]);
  });

  it("pictures over the note's editor go into the note", async () => {
    s.show("notes");
    s.flow.notes = { takes: () => true, open: () => Promise.resolve() };
    s.under({ area: "editor", pictures: true });
    await s.flow.drop(["C:/p.png"], 1, 1);
    expect(s.pictures).toHaveLength(1);
    expect(s.mock.probe.drops.opened).toEqual([]);
  });

  it("the Notes gone (plugin off): everything opens as tabs", async () => {
    s.show("notes");
    s.flow.notes = null;
    await s.flow.drop(["C:/a.md"], 1, 1);
    expect(s.mock.probe.drops.opened).toEqual([{ paths: ["C:/a.md"], scope: "tabs" }]);
  });
});

describe("the view goes", () => {
  it("hidden (a tab switch mid-drag): the badge, the frame, the questions and the tree's mark end", async () => {
    const paths = Array.from({ length: 9 }, (_, n) => `C:/many/${n}.md`);
    await s.flow.drop(paths, 1, 1);
    s.flow.over(1, 1, ["C:/a.md"]);
    s.log.length = 0;
    s.flow.hidden();
    expect(s.flow.hover).toBeNull();
    expect(s.queued).toHaveLength(0);
    expect(s.flow.ask.status).toBe("none");
    expect(s.log).toEqual(["tree.end null", "imports.cancel"]);
    // the drop that comes after (it went to another view) finds nothing pending
    s.flush();
    expect(s.flow.hover).toBeNull();
  });

  it("teardown leaves nothing running", () => {
    s.flow.over(1, 1, ["C:/a.md"]);
    s.flow.reset();
    expect(s.queued).toHaveLength(0);
    expect(s.flow.hover).toBeNull();
  });

  it("events go where their type says", () => {
    s.flow.event({ type: "over", count: 1, paths: ["C:/a.md"], x: 3, y: 4 });
    expect(s.queued).toHaveLength(1);
    s.flow.event({ type: "leave" });
    expect(s.queued).toHaveLength(0);
  });
});
