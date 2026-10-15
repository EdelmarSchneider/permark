// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";

// A file is not a project: inside the open project it opens there, anywhere else it is a loose file of the Notes window, and
// the window that was asked stays as it is. The backend and the system are replaced.
const fake = vi.hoisted(() => ({
  classifyPath: vi.fn(),
  showNotes: vi.fn(),
  openVault: vi.fn(),
  initialPath: vi.fn(),
  launchMode: vi.fn(),
  saveSession: vi.fn(),
  recentVaults: vi.fn(),
  notesOverview: vi.fn(),
}));
vi.mock("../api", () => ({
  api: {
    ...fake,
    appDistribution: async () => "standard",
    metaIndex: async () => [],
    on: async () => () => {},
  },
}));
const platform = vi.hoisted(() => ({
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
vi.mock("../platform", () => platform);

import { app } from "./app.svelte";
import { docs } from "./docs.svelte";
import { toast } from "./toast.svelte";
import { updates } from "./updates.svelte";

describe("opening a file", () => {
  beforeEach(() => {
    for (const mock of Object.values(fake)) mock.mockReset();
    platform.closeThisWindow.mockClear();
    fake.showNotes.mockResolvedValue(undefined);
    fake.saveSession.mockResolvedValue(undefined);
    fake.recentVaults.mockResolvedValue([]);
    fake.notesOverview.mockResolvedValue(null);
    fake.launchMode.mockResolvedValue({ notes: false });
    toast.clear();
    app.screen = "welcome";
    app.openError = null;
    vi.spyOn(updates, "scheduleStartupCheck").mockResolvedValue();
    // the page of the test has no color scheme to watch
    window.matchMedia = vi.fn(() => ({
      matches: false,
      addEventListener: () => {},
      removeEventListener: () => {},
    })) as unknown as typeof window.matchMedia;
  });

  it("hands a file to the Notes window, and opens nothing in this one", async () => {
    fake.classifyPath.mockResolvedValue({ kind: "file", inProject: null });
    expect(await app.openProject("C:/Users/ana/Downloads/proposal.md")).toBe(true);
    expect(fake.showNotes).toHaveBeenCalledWith(false, null, "C:/Users/ana/Downloads/proposal.md");
    expect(fake.openVault).not.toHaveBeenCalled();
    expect(app.screen).toBe("welcome");
  });

  it("shows a file of the open project in the project, not in the Notes window", async () => {
    app.screen = "project";
    fake.classifyPath.mockResolvedValue({ kind: "file", inProject: "docs/plan.md" });
    const open = vi.spyOn(docs, "open").mockResolvedValue(true);
    expect(await app.openProject("C:/work/docs/plan.md")).toBe(true);
    expect(open).toHaveBeenCalledWith("docs/plan.md", expect.anything());
    expect(fake.showNotes).not.toHaveBeenCalled();
  });

  it("says why a file could not be handed over", async () => {
    app.screen = "project";
    fake.classifyPath.mockResolvedValue({ kind: "file", inProject: null });
    fake.showNotes.mockRejectedValue(new Error("no socket"));
    expect(await app.openProject("C:/x/y.md")).toBe(false);
    expect(toast.current).toMatchObject({ type: "message", tone: "warn" });
  });

  it("still opens a folder as a project", async () => {
    fake.classifyPath.mockResolvedValue({ kind: "folder", inProject: null });
    fake.openVault.mockRejectedValue(new Error("not here"));
    expect(await app.openProject("C:/work")).toBe(false);
    expect(fake.openVault).toHaveBeenCalledWith("C:/work");
    expect(fake.showNotes).not.toHaveBeenCalled();
  });

  it("closes a window that was started with a file once the Notes window has it", async () => {
    fake.initialPath.mockResolvedValue("C:/Users/ana/Downloads/proposal.md");
    fake.classifyPath.mockResolvedValue({ kind: "file", inProject: null });
    await app.boot();
    expect(fake.showNotes).toHaveBeenCalledWith(false, null, "C:/Users/ana/Downloads/proposal.md");
    expect(platform.closeThisWindow).toHaveBeenCalled();
  });

  it("keeps a window whose file could not be handed over", async () => {
    fake.initialPath.mockResolvedValue("C:/x/y.md");
    fake.classifyPath.mockResolvedValue({ kind: "file", inProject: null });
    fake.showNotes.mockRejectedValue(new Error("no socket"));
    await app.boot();
    expect(platform.closeThisWindow).not.toHaveBeenCalled();
    expect(app.screen).toBe("welcome");
  });
});
