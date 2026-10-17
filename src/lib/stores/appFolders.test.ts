// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";

// Switching the folder of a window, closing it to the welcome screen, and keeping the list of recent folders. The backend and
// the system are replaced; these tests are about what the app decides.
const fake = vi.hoisted(() => ({
  classifyPath: vi.fn(),
  openVault: vi.fn(),
  closeVault: vi.fn(),
  recentVaults: vi.fn(),
  notesOverview: vi.fn(),
  saveSession: vi.fn(),
  loadSession: vi.fn(),
  listTree: vi.fn(),
  gitSnapshot: vi.fn(),
  stateLocation: vi.fn(),
  sinceLastVisit: vi.fn(),
  launchShow: vi.fn(),
  takeAgentOpen: vi.fn(),
  agentStatus: vi.fn(),
}));
vi.mock("../api", () => ({
  api: {
    ...fake,
    appDistribution: async () => "standard",
    metaIndex: async () => [],
    listTags: async () => ({ tags: [], untagged: 0, missing: 0, status: {} }),
    getUnread: async () => [],
    listComments: async () => [],
    referencedImages: async () => [],
    on: async () => () => {},
  },
}));
vi.mock("../platform", () => ({
  setWindowTitle: vi.fn(async () => {}),
  askUnsaved: vi.fn(async () => "keep"),
  askYesNo: vi.fn(async () => true),
  copyText: vi.fn(async () => true),
  guardWindowClose: vi.fn(async () => () => {}),
  watchFileDrop: vi.fn(async () => () => {}),
  pickFile: vi.fn(async (): Promise<string | null> => null),
  pickFolder: vi.fn(async (): Promise<string | null> => null),
  openExternalUrl: vi.fn(async () => {}),
  closeThisWindow: vi.fn(async () => {}),
  checkForUpdate: vi.fn(),
  assetUrl: (p: string) => p,
  isMock: true,
}));

import { app } from "./app.svelte";
import { edit } from "./edit.svelte";
import { session } from "./session.svelte";
import { toast } from "./toast.svelte";
import { ui } from "./ui.svelte";
import { vault } from "./vault.svelte";
import type { RecentVault, VaultInfo } from "../types";

const info = (root: string, name: string): VaultInfo => ({
  root,
  name,
  kind: "project",
  mode: "ide",
  initialFile: null,
  worktrees: [],
  liveUpdates: true,
  treeDefault: null,
});

const recent = (root: string, name: string): RecentVault => ({ root, name, lastOpenedMs: 1 });
const ALPHA = info("F:\\work\\alpha", "alpha");
const BETA = info("F:\\work\\beta", "beta");
const RECENTS = [recent(BETA.root, "beta"), recent(ALPHA.root, "alpha")];

const tree = { name: "", path: "", kind: "dir", children: [] };

/** A window that shows `ALPHA`. */
function showAlpha(): void {
  app.screen = "project";
  vault.reset();
  vault.info = ALPHA;
  session.load(null);
}

beforeEach(() => {
  vi.restoreAllMocks();
  for (const mock of Object.values(fake)) mock.mockReset();
  fake.classifyPath.mockResolvedValue({ kind: "folder", inProject: null });
  fake.closeVault.mockResolvedValue(undefined);
  fake.recentVaults.mockResolvedValue(RECENTS);
  fake.notesOverview.mockResolvedValue(null);
  fake.saveSession.mockResolvedValue(undefined);
  fake.loadSession.mockResolvedValue(null);
  fake.listTree.mockResolvedValue(tree);
  fake.gitSnapshot.mockResolvedValue({ isRepo: false, files: {}, baseline: { kind: "lastVisit" } });
  fake.stateLocation.mockResolvedValue("appData");
  fake.sinceLastVisit.mockResolvedValue(null);
  fake.takeAgentOpen.mockResolvedValue(null);
  fake.launchShow.mockResolvedValue(null);
  toast.clear();
  ui.folderSwitcher = null;
  app.openError = null;
  app.opening = false;
  app.hiddenRecents.clear();
  app.missingRecents.clear();
  localStorage.clear();
  showAlpha();
});

describe("switching folders", () => {
  it("opens the other folder in this window, the way Open folder does", async () => {
    fake.openVault.mockResolvedValue(BETA);
    expect(await app.switchFolder(BETA.root)).toBe(true);
    expect(fake.openVault).toHaveBeenCalledWith(BETA.root);
    expect(vault.info).toEqual(BETA);
    expect(app.screen).toBe("project");
  });

  it("does nothing for the folder that is already open, however its path is written", async () => {
    expect(await app.switchFolder("f:/work/alpha/")).toBe(false);
    expect(fake.openVault).not.toHaveBeenCalled();
    expect(vault.info).toEqual(ALPHA);
  });

  it("waits for the answer when there are unsaved edits, and loses nothing", async () => {
    fake.openVault.mockResolvedValue(BETA);
    let resume: (() => void) | undefined;
    const hold = vi.spyOn(edit, "holdNavigation").mockImplementation((go) => {
      resume = go;
      return true;
    });
    expect(await app.switchFolder(BETA.root)).toBe(false);
    expect(hold).toHaveBeenCalledTimes(1);
    // the person has not answered: the folder that is open stays, with its text
    expect(fake.openVault).not.toHaveBeenCalled();
    expect(vault.info).toEqual(ALPHA);
    // "Save" (or "Discard") carries the move on
    hold.mockReturnValue(false);
    resume?.();
    await vi.waitFor(() => expect(vault.info).toEqual(BETA));
  });

  it("keeps the open folder when the other one cannot be opened, and says so", async () => {
    fake.openVault.mockRejectedValue({ kind: "notFound", subject: BETA.root });
    expect(await app.switchFolder(BETA.root)).toBe(false);
    expect(vault.info).toEqual(ALPHA);
    expect(app.screen).toBe("project");
    expect(toast.current).toMatchObject({ type: "message", tone: "warn" });
    // and the lists mark the folder as not found, until it opens
    expect(app.missingRecents.has(BETA.root)).toBe(true);
    fake.openVault.mockResolvedValue(BETA);
    expect(await app.switchFolder(BETA.root)).toBe(true);
    expect(app.missingRecents.has(BETA.root)).toBe(false);
  });
});

describe("closing the folder", () => {
  it("goes back to the welcome screen with the recent folders listed", async () => {
    await app.closeFolder();
    expect(fake.closeVault).toHaveBeenCalledTimes(1);
    expect(app.screen).toBe("welcome");
    expect(app.recents).toEqual(RECENTS);
    expect(vault.info).toBeNull();
    expect(app.opening).toBe(false);
  });

  it("saves the session of the folder before it is closed", async () => {
    const order: string[] = [];
    fake.saveSession.mockImplementation(async () => void order.push("save"));
    fake.closeVault.mockImplementation(async () => void order.push("close"));
    await app.closeFolder();
    expect(order).toEqual(["save", "close"]);
  });

  it("is asked about unsaved edits first, and closes nothing until they are answered", async () => {
    let resume: (() => void) | undefined;
    const hold = vi.spyOn(edit, "holdNavigation").mockImplementation((go) => {
      resume = go;
      return true;
    });
    await app.closeFolder();
    expect(fake.closeVault).not.toHaveBeenCalled();
    expect(app.screen).toBe("project");
    hold.mockReturnValue(false);
    resume?.();
    await vi.waitFor(() => expect(app.screen).toBe("welcome"));
    expect(fake.closeVault).toHaveBeenCalledTimes(1);
  });

  it("stays in the folder, and says why, when the backend cannot close it", async () => {
    fake.closeVault.mockRejectedValue({ kind: "internal", detail: "boom" });
    await app.closeFolder();
    expect(app.screen).toBe("project");
    expect(vault.info).toEqual(ALPHA);
    expect(toast.current).toMatchObject({ type: "message", tone: "warn" });
    expect(app.opening).toBe(false);
  });

  it("lets go of the folder's views, so the next folder starts clean", async () => {
    ui.sidebarView = "search";
    ui.folderSwitcher = { anchor: { left: 0, right: 0, top: 0, bottom: 0 } };
    await app.closeFolder();
    expect(ui.sidebarView).toBe("explorer");
    expect(ui.folderSwitcher).toBeNull();
  });

  it("does nothing outside a project", async () => {
    app.screen = "welcome";
    await app.closeFolder();
    expect(fake.closeVault).not.toHaveBeenCalled();
  });

  it("can open a folder again from the welcome screen it went back to", async () => {
    fake.openVault.mockResolvedValue(BETA);
    await app.closeFolder();
    expect(await app.openProject(BETA.root)).toBe(true);
    expect(app.screen).toBe("project");
    expect(vault.info).toEqual(BETA);
  });
});

describe("the folder switcher menu", () => {
  const anchor = { left: 10, right: 90, top: 30, bottom: 50 };

  it("reads the list of recent folders again when it opens, so the folder on screen is in it", async () => {
    app.recents = [];
    await app.openFolderSwitcher(anchor);
    expect(fake.recentVaults).toHaveBeenCalled();
    expect(app.recents).toEqual(RECENTS);
    expect(ui.folderSwitcher).toEqual({ anchor });
  });

  it("keeps the list it has when the backend cannot give a new one", async () => {
    app.recents = RECENTS;
    fake.recentVaults.mockRejectedValue(new Error("no file"));
    await app.openFolderSwitcher(anchor);
    expect(app.recents).toEqual(RECENTS);
    expect(ui.folderSwitcher).not.toBeNull();
  });

  it("closes when it is asked for again", async () => {
    await app.openFolderSwitcher(anchor);
    await app.openFolderSwitcher(anchor);
    expect(ui.folderSwitcher).toBeNull();
  });

  it("opens only in a project, not on the welcome screen or in the Notes window", async () => {
    app.screen = "welcome";
    await app.openFolderSwitcher(anchor);
    expect(ui.folderSwitcher).toBeNull();
    app.screen = "project";
    vault.info = { ...ALPHA, kind: "notes" };
    await app.openFolderSwitcher(anchor);
    expect(ui.folderSwitcher).toBeNull();
  });

  it("is a command that opens the menu and a command that closes the folder", async () => {
    app.run("switchFolder");
    await vi.waitFor(() => expect(ui.folderSwitcher).not.toBeNull());
    ui.folderSwitcher = null;
    app.run("closeFolder");
    await vi.waitFor(() => expect(app.screen).toBe("welcome"));
  });
});

describe("removing a recent folder", () => {
  it("forgets the entry only, and remembers it across launches", () => {
    app.recents = RECENTS;
    app.removeRecent(BETA.root);
    expect(app.visibleRecents.map((r) => r.root)).toEqual([ALPHA.root]);
    // the list is the backend's: nothing was asked of the disk
    expect(fake.openVault).not.toHaveBeenCalled();
    expect(JSON.parse(localStorage.getItem("permark.recents.hidden") ?? "[]")).toEqual([
      BETA.root,
    ]);
  });

  it("brings the folder back into the list when it is opened again", async () => {
    app.recents = RECENTS;
    app.removeRecent(BETA.root);
    fake.openVault.mockResolvedValue(BETA);
    await app.openProject(BETA.root);
    expect(app.hiddenRecents.has(BETA.root)).toBe(false);
    expect(JSON.parse(localStorage.getItem("permark.recents.hidden") ?? "[]")).toEqual([]);
  });

  it("leaves the other removed folders removed when one is opened again", async () => {
    app.removeRecent(BETA.root);
    app.removeRecent("D:\\old\\gamma");
    fake.openVault.mockResolvedValue(BETA);
    await app.openProject(BETA.root);
    expect([...app.hiddenRecents]).toEqual(["D:\\old\\gamma"]);
  });
});
