// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Api, ApiError, DocumentView, WorkspaceView } from "../../api";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { DocumentController } from "./document.svelte";
import { reading } from "./reading.svelte";

type Mock = ReturnType<typeof createMockApi>;

type Fx = {
  api: Mock;
  doc: DocumentController;
  workspace: WorkspaceView;
  notes: string[];
  failures: ApiError[];
  revealed: string[];
  /** What the window does with `window-changed`: the document follows. */
  follow: () => Promise<void>;
};

async function setup(): Promise<Fx> {
  const api = createMockApi();
  await api.openPath(MOCK_FOLDER, "thisTab");
  const boot = await api.windowBoot();
  if (!boot.ok || boot.value.view.state !== "showing") throw new Error("no workspace");
  const workspace = boot.value.view.workspace;
  const doc = new DocumentController(api);
  const notes: string[] = [];
  const failures: ApiError[] = [];
  const revealed: string[] = [];
  doc.bind({
    note: (text) => notes.push(text),
    fail: (error) => failures.push(error),
    reveal: (path) => revealed.push(path),
  });
  doc.setWorkspace(workspace);
  // the window tells the document what to show, as the real window does on `window-changed`
  await api.on("window-changed", (event) => {
    if (event.view.state === "showing") void doc.show(event.view.document);
  });
  return { api, doc, workspace, notes, failures, revealed, follow: () => tick() };
}

const tick = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0));

async function open(fx: Fx, path: string): Promise<void> {
  await fx.api.showDocument(path);
  await tick();
}

function markdownIds(view: DocumentView | null): string[] {
  return view?.body.kind === "markdown" ? view.body.blocks.map((b) => b.id) : [];
}

beforeEach(() => {
  vi.stubGlobal("navigator", { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } });
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("opening", () => {
  it("shows the document the window shows, starting with nothing", async () => {
    const fx = await setup();
    expect(fx.doc.state.status).toBe("empty");
    await open(fx, "README.md");
    expect(fx.doc.state.status).toBe("ready");
    expect(fx.doc.path).toBe("README.md");
    expect(fx.doc.view?.body.kind).toBe("markdown");
    expect(fx.api.probe.shown.path).toBe("README.md");
    expect(fx.api.probe.shown.known).toEqual([]);
  });

  it("keeps what was on screen while the next one loads, and tells a failure", async () => {
    const fx = await setup();
    await open(fx, "README.md");
    const before = fx.doc.view;
    const pending = fx.doc.show("docs/guide.md");
    expect(fx.doc.state.status).toBe("loading");
    expect(fx.doc.view).toBe(before);
    await pending;
    expect(fx.doc.view?.path).toBe("docs/guide.md");
    await fx.doc.show("missing.md");
    expect(fx.doc.state.status).toBe("failed");
    expect(fx.doc.view).toBeNull();
    if (fx.doc.state.status === "failed") {
      expect(fx.doc.state.error.kind === "backend" && fx.doc.state.error.error.kind).toBe(
        "notFound",
      );
    }
  });

  it("a second ask for the document that is being opened waits for that open", async () => {
    const fx = await setup();
    const first = fx.doc.show("docs/guide.md");
    let second = "waiting";
    const asked = fx.doc.show("docs/guide.md").then(() => {
      second = fx.doc.state.status;
    });
    expect(second).toBe("waiting");
    await asked;
    await first;
    expect(second).toBe("ready");
    expect(fx.doc.view?.path).toBe("docs/guide.md");
  });

  it("a newer open wins over a slower one", async () => {
    const fx = await setup();
    const slow = vi.spyOn(fx.api, "openDocument");
    slow.mockImplementationOnce(
      (path, known) =>
        new Promise((resolve) => {
          setTimeout(() => {
            slow.mockRestore();
            void fx.api.openDocument(path, known).then(resolve);
          }, 30);
        }),
    );
    const first = fx.doc.show("README.md");
    await fx.doc.show("docs/guide.md");
    await first;
    expect(fx.doc.view?.path).toBe("docs/guide.md");
  });

  it("closing the document empties the state and tells the backend", async () => {
    const fx = await setup();
    await open(fx, "README.md");
    await fx.doc.show(null);
    expect(fx.doc.state.status).toBe("empty");
    expect(fx.api.probe.shown.path).toBeNull();
  });

  it("another workspace starts a new session", async () => {
    const fx = await setup();
    await open(fx, "README.md");
    fx.doc.setWorkspace({ ...fx.workspace, id: fx.workspace.id + 1 });
    expect(fx.doc.state.status).toBe("empty");
    expect(fx.doc.trail.canBack).toBe(false);
  });
});

describe("changes on disk", () => {
  it("asks again for the blocks it does not hold", async () => {
    const fx = await setup();
    await open(fx, "README.md");
    const view = fx.doc.view;
    const ids = markdownIds(view);
    expect(ids.length).toBeGreaterThan(5);
    // the page holds these blocks
    vi.spyOn(fx.doc.host, "ids").mockReturnValue(ids);
    fx.doc.onChanged({ window: "main", path: "README.md", change: "changed", version: "v2" });
    await tick();
    expect(fx.api.probe.shown.known).toEqual(ids);
    const next = fx.doc.state.status === "ready" ? fx.doc.state.view : null;
    expect(next?.body.kind === "markdown" && next.body.blocks.every((b) => b.html === null)).toBe(
      true,
    );
    expect(fx.doc.state.status === "ready" && fx.doc.state.fresh).toBe(false);
  });

  it("ignores changes of other documents", async () => {
    const fx = await setup();
    await open(fx, "README.md");
    const seen = vi.spyOn(fx.api, "openDocument");
    fx.doc.onChanged({ window: "main", path: "docs/guide.md", change: "changed", version: "x" });
    await tick();
    expect(seen).not.toHaveBeenCalled();
  });

  it("keeps the last version when the file is deleted, and says where it went", async () => {
    const fx = await setup();
    await open(fx, "README.md");
    const view = fx.doc.view;
    fx.doc.onChanged({
      window: "main",
      path: "README.md",
      change: "deleted",
      movedTo: "docs/README.md",
    });
    expect(fx.doc.state.status).toBe("ready");
    expect(fx.doc.view).toBe(view);
    expect(fx.doc.state.status === "ready" && fx.doc.state.deleted?.movedTo).toBe("docs/README.md");
    // the file is back: the deleted mark goes with the new version
    fx.doc.onChanged({ window: "main", path: "README.md", change: "changed", version: "v3" });
    await tick();
    expect(fx.doc.state.status === "ready" && fx.doc.state.deleted).toBeNull();
  });
});

describe("links and navigation", () => {
  it("opens a document link, with its history", async () => {
    const fx = await setup();
    await open(fx, "README.md");
    await fx.doc.activateLink("docs/guide.md#getting-started");
    await tick();
    expect(fx.doc.path).toBe("docs/guide.md");
    expect(fx.doc.trail.canBack).toBe(true);
    await fx.doc.back();
    await tick();
    expect(fx.doc.path).toBe("README.md");
    expect(fx.doc.trail.canForward).toBe(true);
    await fx.doc.forward();
    await tick();
    expect(fx.doc.path).toBe("docs/guide.md");
  });

  it("back through the trail does not add places", async () => {
    const fx = await setup();
    await open(fx, "README.md");
    await fx.doc.activateLink("docs/guide.md");
    await tick();
    await fx.doc.back();
    await tick();
    await fx.doc.back();
    expect(fx.doc.trail.recent().map((p) => p.place.path)).toEqual(["docs/guide.md", "README.md"]);
  });

  it("sends web links to the system browser and tells a missing target", async () => {
    const fx = await setup();
    await open(fx, "README.md");
    await fx.doc.activateLink("https://example.com/page");
    await fx.doc.activateLink("mailto:me@example.com");
    expect(fx.api.probe.web).toEqual(["https://example.com/page", "mailto:me@example.com"]);
    await fx.doc.activateLink("nope.md");
    expect(fx.notes.at(-1)).toContain("nope.md");
    // other schemes do nothing at all
    await fx.doc.activateLink("javascript:alert(1)");
    await fx.doc.activateLink("file:///C:/x");
    expect(fx.api.probe.web).toHaveLength(2);
    expect(fx.api.probe.opened).toEqual([]);
  });

  it("opens assets in the app or the system's program, folders in the tree", async () => {
    const fx = await setup();
    await open(fx, "docs/guide.md");
    await fx.doc.activateLink("img/logo.png");
    await tick();
    expect(fx.doc.path).toBe("docs/img/logo.png");
    expect(fx.doc.view?.body.kind).toBe("media");
    await fx.doc.activateLink("../../package.json");
    await tick();
    expect(fx.doc.path).toBe("package.json");
  });

  it("an anchor of the same document stays in it", async () => {
    const fx = await setup();
    await open(fx, "README.md");
    const show = vi.spyOn(fx.api, "showDocument");
    await fx.doc.activateLink("#diagrams");
    expect(show).not.toHaveBeenCalled();
    // not built in this test (no page): told, not thrown
    expect(fx.notes.at(-1)).toContain("diagrams");
  });
});

describe("copying", () => {
  it("copies a reference with the template and the whole document as text", async () => {
    const fx = await setup();
    await open(fx, "docs/guide.md");
    const write = vi.mocked(navigator.clipboard.writeText);
    await fx.doc.copyReference("@{path}");
    expect(write).toHaveBeenLastCalledWith("@docs/guide.md");
    await fx.doc.copyReference("{path}#{section}");
    expect(write).toHaveBeenLastCalledWith("docs/guide.md");
    await fx.doc.copyDocument();
    expect(write).toHaveBeenLastCalledWith("# docs/guide.md\n");
  });

  it("tells when the clipboard refuses", async () => {
    const fx = await setup();
    await open(fx, "README.md");
    vi.mocked(navigator.clipboard.writeText).mockRejectedValueOnce(new Error("denied"));
    await fx.doc.copyReference("{path}");
    expect(fx.notes).toHaveLength(1);
  });

  it("the reference of a passage names the heading above it and its line", async () => {
    const fx = await setup();
    await open(fx, "README.md");
    const view = fx.doc.view;
    if (view?.body.kind !== "markdown") throw new Error("not markdown");
    const code = view.body.blocks.find((b) => b.html?.includes("language-rust"));
    const item = view.body.outline.find((o) => o.label === "Code");
    expect(code).toBeDefined();
    const element = document.createElement("p");
    element.setAttribute("data-b", code?.id ?? "");
    document.body.append(element);
    const ref = fx.doc.referenceAt(element);
    expect(ref).toEqual({ path: "README.md", section: item?.id, line: code?.lineStart });
    // outside any block: the document alone
    expect(fx.doc.referenceAt(null)).toEqual({ path: "README.md", section: null, line: null });
  });
});

describe("the editor", () => {
  it("opens the document and code references in the editor, with the line", async () => {
    const fx = await setup();
    await open(fx, "README.md");
    await fx.doc.openInEditor();
    await fx.doc.openCodeRef({ paths: ["src/main.rs"], line: 12, col: 3 }, "src/main.rs");
    expect(fx.api.probe.editor).toEqual([
      { path: "README.md", line: null, col: null },
      { path: "src/main.rs", line: 12, col: 3 },
    ]);
  });

  it("tells a failed launch", async () => {
    const fx = await setup();
    await open(fx, "README.md");
    const failing: Api["openInEditor"] = () =>
      Promise.resolve({
        ok: false,
        error: {
          kind: "backend",
          command: "open_in_editor",
          error: { kind: "editorNotSet", subject: null, detail: null },
        },
      });
    vi.spyOn(fx.api, "openInEditor").mockImplementation(failing);
    await fx.doc.openInEditor();
    expect(fx.failures).toHaveLength(1);
  });
});

describe("where the reader is", () => {
  it("remembers a place per document and gives it back", () => {
    reading.remember("a.md", 420.4);
    reading.remember("b.md", 0);
    expect(reading.scrollOf("a.md")).toBe(420);
    expect(reading.scrollOf("b.md")).toBeNull();
    expect(reading.scrollOf("c.md")).toBeNull();
  });

  it("keeps the places of the last 50 documents", () => {
    for (let i = 0; i < 60; i++) reading.remember(`d${i}.md`, 10 + i);
    expect(reading.scrollOf("d0.md")).toBeNull();
    expect(reading.scrollOf("d59.md")).toBe(69);
  });
});
