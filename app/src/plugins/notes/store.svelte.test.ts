// @vitest-environment jsdom
// The Notes page attaches the store from its effect. Whatever the reading side and the editor read while it attaches
// must not become a dependency of that effect: a change of it (the editor opening on a new note) would attach again,
// take the page and its editor away and open them again, without end.
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, expect, it } from "vitest";
import { createMockApi } from "../../api/mock";
import type { WorkspaceView } from "../../api";
import { DocumentController } from "../../core/doc/document.svelte";
import { EditingSession } from "../../core/editing/session.svelte";
import { PluginClient } from "../../core/plugins/client";
import { WindowStore } from "../../core/window.svelte";
import { NotesStore } from "./store.svelte";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 6; i += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

const offs: (() => void)[] = [];
afterEach(() => {
  for (const off of offs.splice(0)) off();
});

it("a new note opens in the editor, and the page attached from an effect stays attached", async () => {
  const api = createMockApi();
  const outcome = await api.openNotes("thisTab");
  if (!outcome.ok || outcome.value.outcome !== "opened") throw new Error("notes did not open");
  const view = outcome.value.view;
  if (view.state !== "showing" || view.workspace.kind !== "notes") throw new Error("not notes");
  const workspace: Extract<WorkspaceView, { kind: "notes" }> = view.workspace;
  const documents = new DocumentController(api);
  const editing = new EditingSession(api);
  const store = new NotesStore({
    api,
    client: new PluginClient(api),
    documents,
    editing,
    window: new WindowStore(api),
  });
  // the document area as the page binds it: a pane on screen, whose scroll belongs to the editor while one shows
  const pane = document.createElement("div");
  documents.attachBody(pane, pane);
  documents.bind({
    note: () => {},
    pageNote: () => {},
    clearPageNote: () => {},
    fail: () => {},
    reveal: () => {},
    editorSet: () => false,
    modKey: () => "Ctrl",
    chooseEditor: () => {},
    editorShown: () => editing.mode !== "view",
    editorHeadings: () => null,
    showEditorHeading: () => false,
    showEditorLine: () => false,
    openFolderWith: () => {},
  });
  let attached = 0;
  offs.push(
    $effect.root(() => {
      $effect(() => {
        attached += 1;
        return store.attach(workspace);
      });
    }),
    () => editing.release(),
  );
  flushSync();
  await settle();

  await store.newNote();
  await settle();

  expect(attached).toBe(1);
  expect(store.current).not.toBeNull();
  expect(editing.path).toBe(store.current);
  expect(editing.mode).not.toBe("view");
});
