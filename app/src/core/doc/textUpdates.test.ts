// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Api, DocumentChanged, DocumentView, WorkspaceView } from "../../api";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { DocumentController, type TextUpdate } from "./document.svelte";

const tick = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0));

function textView(path: string, text: string, partial = false): DocumentView {
  return {
    path,
    name: path,
    format: "text",
    role: null,
    body: {
      kind: "text",
      text,
      language: "rs",
      ...(partial ? { partial: { loaded: text.length, total: text.length * 2 } } : {}),
    },
    hash: "0123456789abcdef",
    size: text.length,
    mtimeMs: 1,
    editable: { editable: "surgical" },
    notes: [],
  };
}

/** A controller over a backend whose document is whatever `files` says. */
async function setup(files: Map<string, DocumentView>) {
  const mock = createMockApi();
  await mock.openPath(MOCK_FOLDER, "thisTab");
  const boot = await mock.windowBoot();
  if (!boot.ok || boot.value.view.state !== "showing") throw new Error("no workspace");
  const workspace: WorkspaceView = boot.value.view.workspace;
  const api: Api = {
    ...mock,
    openDocument: (path) => {
      const view = files.get(path);
      return Promise.resolve(view ? { ok: true, value: view } : mock.openDocument(path, []));
    },
  };
  const doc = new DocumentController(api);
  doc.setWorkspace(workspace);
  const updates: TextUpdate[] = [];
  const off = doc.onTextUpdated((update) => updates.push(update));
  const changed = async (path: string): Promise<void> => {
    const event: DocumentChanged = { window: "w", path, change: "changed", version: "v" };
    doc.onChanged(event);
    await tick();
  };
  return { doc, updates, off, changed };
}

afterEach(() => vi.useRealTimers());

describe("a text document read again", () => {
  it("tells which lines are different now, and where lines went", async () => {
    const files = new Map([["a.rs", textView("a.rs", "one\ntwo\nthree\nfour\n")]]);
    const { doc, updates, changed } = await setup(files);
    await doc.show("a.rs");
    // a fresh open glows nothing
    expect(updates).toEqual([]);
    files.set("a.rs", textView("a.rs", "one\nTWO\nthree\nfive\nsix\n"));
    await changed("a.rs");
    expect(updates).toEqual([
      {
        path: "a.rs",
        added: [
          { first: 2, last: 2 },
          { first: 4, last: 5 },
        ],
        removedAfter: [],
      },
    ]);
    files.set("a.rs", textView("a.rs", "one\nthree\nfive\nsix\n"));
    await changed("a.rs");
    expect(updates.at(-1)).toEqual({ path: "a.rs", added: [], removedAfter: [1] });
  });

  it("says nothing when the text is the same, when it is partly loaded, or when nobody listens", async () => {
    const files = new Map([["a.rs", textView("a.rs", "one\ntwo\n")]]);
    const { doc, updates, changed, off } = await setup(files);
    await doc.show("a.rs");
    files.set("a.rs", textView("a.rs", "one\ntwo\n"));
    await changed("a.rs");
    expect(updates).toEqual([]);
    // a large file shows only its start: no lines to compare
    files.set("a.rs", textView("a.rs", "one\ntwo\nthree\n", true));
    await changed("a.rs");
    files.set("a.rs", textView("a.rs", "one\nTWO\nthree\n", true));
    await changed("a.rs");
    expect(updates).toEqual([]);
    // stopped listening
    off();
    files.set("a.rs", textView("a.rs", "x\n"));
    await changed("a.rs");
    expect(updates).toEqual([]);
  });

  it("does not light the lines the person saved themselves", async () => {
    const files = new Map([["a.rs", textView("a.rs", "one\ntwo\n")]]);
    const { doc, updates } = await setup(files);
    await doc.show("a.rs");
    files.set("a.rs", textView("a.rs", "one\nTWO\n"));
    doc.reload();
    await tick();
    expect(doc.view?.body).toMatchObject({ text: "one\nTWO\n" });
    expect(updates).toEqual([]);
  });

  it("does not compare one document with another", async () => {
    const files = new Map([
      ["a.rs", textView("a.rs", "one\n")],
      ["b.rs", textView("b.rs", "one\ntwo\n")],
    ]);
    const { doc, updates } = await setup(files);
    await doc.show("a.rs");
    await doc.show("b.rs");
    expect(updates).toEqual([]);
  });
});
