import { beforeEach, describe, expect, it, vi } from "vitest";

// The backend and the system are replaced: these tests are about where Alt+U goes and what it says.
vi.mock("../api", () => ({
  api: { saveSession: vi.fn(async () => {}), on: async () => () => {} },
}));
vi.mock("../platform", () => ({
  setWindowTitle: vi.fn(async () => {}),
  askUnsaved: vi.fn(async () => "keep"),
  askYesNo: vi.fn(async () => true),
  copyText: vi.fn(async () => true),
  guardWindowClose: vi.fn(async () => () => {}),
  watchFileDrop: vi.fn(async () => () => {}),
  checkForUpdate: vi.fn(),
  assetUrl: (p: string) => p,
  isMock: true,
}));

import type { TreeNode } from "../types";
import { app } from "./app.svelte";
import { docs } from "./docs.svelte";
import { feed } from "./feed.svelte";
import { toast } from "./toast.svelte";
import { unread } from "./unread.svelte";
import { vault } from "./vault.svelte";

const file = (path: string): TreeNode => ({ name: path, path, kind: "file" });

describe("Alt+U", () => {
  let opened: string[];

  beforeEach(() => {
    unread.reset();
    feed.clear();
    toast.clear();
    docs.reset();
    vault.reset();
    vault.tree = { name: "p", path: "", kind: "dir", children: ["a.md", "b.md", "c.md"].map(file) };
    opened = [];
    vi.spyOn(docs, "open").mockImplementation(async (path) => {
      opened.push(path);
      return true;
    });
  });

  it("opens the first unread document in tree order", async () => {
    unread.onChanged(["b.md", "c.md"]);
    await app.openNextUnread();
    expect(opened).toEqual(["b.md"]);
  });

  it("opens what changed most recently first", async () => {
    unread.onChanged(["a.md", "c.md"]);
    feed.apply(
      {
        changes: [
          { kind: "modified", path: "a.md", atMs: 1 },
          { kind: "modified", path: "c.md", atMs: 2 },
        ],
        bulk: null,
        rescan: false,
      },
      2,
    );
    await app.openNextUnread();
    expect(opened).toEqual(["c.md"]);
  });

  it("says so when nothing is left", async () => {
    await app.openNextUnread();
    expect(opened).toEqual([]);
    expect(toast.current).toMatchObject({ type: "message", text: "All caught up." });
  });

  it("is a command that does the same", async () => {
    unread.onChanged(["c.md"]);
    app.screen = "project";
    vault.info = { mode: "ide" } as NonNullable<typeof vault.info>;
    app.run("nextUnread");
    await vi.waitFor(() => expect(opened).toEqual(["c.md"]));
    app.screen = "boot";
  });
});
