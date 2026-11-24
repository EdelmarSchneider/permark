// @vitest-environment jsdom
// The Notes page attaches the store once per Notes workspace: the same workspace told again (its folder made by the
// first note) keeps the page and its editor; another one attaches again.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import type { WorkspaceView } from "../../api";

const attach = vi.fn((_workspace: unknown) => () => {});

vi.mock("./store.svelte", () => ({
  notes: { attach: (workspace: unknown) => attach(workspace), current: null, openFiles: () => {} },
}));
// the parts of the page that only draw the store
const nothing = (): void => {};
vi.mock("./NoteBars.svelte", () => ({ default: nothing }));
vi.mock("./VersionsDialog.svelte", () => ({ default: nothing }));
vi.mock("./MoveDialog.svelte", () => ({ default: nothing }));
vi.mock("./ConfirmDialog.svelte", () => ({ default: nothing }));
vi.mock("./CompareDialog.svelte", () => ({ default: nothing }));
vi.mock("../../core/doc/DocumentArea.svelte", () => ({ default: nothing }));

const { default: NotesMain } = await import("./NotesMain.svelte");

let app: Record<string, unknown> | null = null;
afterEach(() => {
  if (app) unmount(app);
  app = null;
  attach.mockClear();
});

type Notes = Extract<WorkspaceView, { kind: "notes" }>;

it("attaches once per Notes workspace, not again when the same one comes with its folder", () => {
  const props = $state<{ workspace: Notes }>({
    workspace: { kind: "notes", id: 2, name: "Notes", folder: null },
  });
  app = mount(NotesMain, { target: document.body, props });
  flushSync();
  expect(attach).toHaveBeenCalledTimes(1);

  props.workspace = {
    kind: "notes",
    id: 2,
    name: "Notes",
    folder: "C:/Users/ana/Documents/PerMark Notes",
  };
  flushSync();
  expect(attach).toHaveBeenCalledTimes(1);

  props.workspace = {
    kind: "notes",
    id: 3,
    name: "Notes",
    folder: "C:/Users/ana/Documents/PerMark Notes",
  };
  flushSync();
  expect(attach).toHaveBeenCalledTimes(2);
});
