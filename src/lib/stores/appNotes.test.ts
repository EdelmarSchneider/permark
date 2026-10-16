// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";

// One window, not two: a window that shows only the welcome screen turns into the Notes window when it asks for it (New note, a
// note of the Notes column, Ctrl+Shift+N, Ctrl+Shift+D), or closes when a Notes window already runs in another program. A window
// with a project open stays as it is. The backend and the system are replaced; nothing waits for time.
const fake = vi.hoisted(() => ({
  becomeNotesWindow: vi.fn(),
  classifyPath: vi.fn(),
  showNotes: vi.fn(),
  initialPath: vi.fn(),
  launchMode: vi.fn(),
  recentVaults: vi.fn(),
  notesOverview: vi.fn(),
  saveSession: vi.fn(),
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
  watchFileDrop: vi.fn(),
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
import { notes } from "./notes.svelte";
import { toast } from "./toast.svelte";
import { ui } from "./ui.svelte";
import { updates } from "./updates.svelte";

let stopListening: ReturnType<typeof vi.fn>;
let startNotes: ReturnType<typeof vi.spyOn>;

beforeEach(async () => {
  for (const mock of Object.values(fake)) mock.mockReset();
  for (const mock of [platform.closeThisWindow, platform.watchFileDrop]) mock.mockReset();
  stopListening = vi.fn();
  platform.watchFileDrop.mockResolvedValue(stopListening);
  platform.closeThisWindow.mockResolvedValue(undefined);
  fake.becomeNotesWindow.mockResolvedValue("became");
  fake.showNotes.mockResolvedValue(undefined);
  fake.initialPath.mockResolvedValue(null);
  fake.launchMode.mockResolvedValue({ notes: false });
  fake.recentVaults.mockResolvedValue([]);
  fake.notesOverview.mockResolvedValue(null);
  fake.saveSession.mockResolvedValue(undefined);
  toast.clear();
  ui.drag = null;
  vi.spyOn(updates, "scheduleStartupCheck").mockResolvedValue();
  startNotes = vi.spyOn(notes, "start").mockResolvedValue();
  // the page of the test has no color scheme to watch
  window.matchMedia = vi.fn(() => ({
    matches: false,
    addEventListener: () => {},
    removeEventListener: () => {},
  })) as unknown as typeof window.matchMedia;
  // the program starts on the welcome screen, with its drop listener on
  app.screen = "boot";
  app.openError = null;
  app.opening = false;
  await app.boot();
  expect(app.screen).toBe("welcome");
  startNotes.mockClear();
});

describe("a window that shows only the welcome screen", () => {
  it("turns into the Notes window for New note, and leaves nothing behind", async () => {
    await app.newNote();
    expect(fake.becomeNotesWindow).toHaveBeenCalledTimes(1);
    expect(fake.becomeNotesWindow).toHaveBeenCalledWith(true, null, null);
    // the same window shows the notes now: no second window was asked for, and this one was not closed
    expect(fake.showNotes).not.toHaveBeenCalled();
    expect(platform.closeThisWindow).not.toHaveBeenCalled();
    expect(app.screen).toBe("notes");
    expect(startNotes).toHaveBeenCalledTimes(1);
  });

  it("turns into the Notes window for Notes, and for a note of the Notes column, which it shows", async () => {
    await app.showNotes();
    expect(fake.becomeNotesWindow).toHaveBeenLastCalledWith(false, null, null);
    expect(app.screen).toBe("notes");

    app.screen = "welcome";
    await app.showNotes(false, "2026-10-15 1432.md");
    expect(fake.becomeNotesWindow).toHaveBeenLastCalledWith(false, "2026-10-15 1432.md", null);
    expect(fake.showNotes).not.toHaveBeenCalled();
  });

  it("does the same for Ctrl+Shift+N and Ctrl+Shift+D", async () => {
    app.run("newNote");
    await vi.waitFor(() => expect(startNotes).toHaveBeenCalledTimes(1));
    expect(fake.becomeNotesWindow).toHaveBeenLastCalledWith(true, null, null);
    // (the answer is taken before the next ask: the welcome screen is put back, as a new window would have it)
    await new Promise((resolve) => setTimeout(resolve, 0));

    app.screen = "welcome";
    app.run("showNotes");
    await vi.waitFor(() => expect(fake.becomeNotesWindow).toHaveBeenCalledTimes(2));
    expect(fake.becomeNotesWindow).toHaveBeenLastCalledWith(false, null, null);
    expect(fake.showNotes).not.toHaveBeenCalled();
  });

  it("stops taking dropped files as projects: the Notes window listens by itself", async () => {
    expect(stopListening).not.toHaveBeenCalled();
    ui.drag = { count: 2 };
    await app.newNote();
    expect(stopListening).toHaveBeenCalledTimes(1);
    expect(ui.drag).toBeNull();
  });

  it("closes itself when a Notes window runs in another program, which was asked to come forward", async () => {
    fake.becomeNotesWindow.mockResolvedValue("forwarded");
    await app.newNote();
    expect(fake.becomeNotesWindow).toHaveBeenCalledWith(true, null, null);
    expect(platform.closeThisWindow).toHaveBeenCalledTimes(1);
    // it did not turn into anything, and still listens for drops until it is gone
    expect(startNotes).not.toHaveBeenCalled();
    expect(app.screen).toBe("welcome");
    expect(stopListening).not.toHaveBeenCalled();
  });

  it("turns into the Notes window with a file it is given (Ctrl+Shift+O, a drop), which opens there", async () => {
    fake.classifyPath.mockResolvedValue({ kind: "file", inProject: null });
    expect(await app.openProject("C:/Users/ana/Downloads/proposal.md")).toBe(true);
    expect(fake.becomeNotesWindow).toHaveBeenCalledWith(
      false,
      null,
      "C:/Users/ana/Downloads/proposal.md",
    );
    expect(fake.showNotes).not.toHaveBeenCalled();
    expect(app.screen).toBe("notes");
  });

  it("stays the welcome screen and says why when the file could not be handed over", async () => {
    fake.classifyPath.mockResolvedValue({ kind: "file", inProject: null });
    fake.becomeNotesWindow.mockRejectedValueOnce(new Error("no socket"));
    expect(await app.openProject("C:/x/y.md")).toBe(false);
    expect(app.screen).toBe("welcome");
    expect(app.openError).toMatch(/C:\/x\/y\.md/);
    expect(app.openError).toMatch(/no socket/);
  });

  it("takes the request once, however many times it is asked while the answer is on its way", async () => {
    let answer: (value: string) => void = () => {};
    fake.becomeNotesWindow.mockReturnValue(
      new Promise<string>((resolve) => {
        answer = resolve;
      }),
    );
    const first = app.newNote();
    await app.newNote();
    app.run("newNote");
    expect(fake.becomeNotesWindow).toHaveBeenCalledTimes(1);
    answer("became");
    await first;
    expect(startNotes).toHaveBeenCalledTimes(1);
    expect(app.screen).toBe("notes");
  });

  it("stays the welcome screen, and says why, when the Notes window cannot be reached", async () => {
    fake.becomeNotesWindow.mockRejectedValueOnce(new Error("no socket"));
    await app.newNote();
    expect(app.screen).toBe("welcome");
    expect(startNotes).not.toHaveBeenCalled();
    expect(app.openError).toMatch(/no socket/);
    expect(stopListening).not.toHaveBeenCalled();
    // and it can be asked again
    await app.newNote();
    expect(app.screen).toBe("notes");
    expect(app.openError).toBeNull();
  });

  it("does not turn into the Notes window while a project is being opened", async () => {
    app.opening = true;
    await app.newNote();
    expect(fake.becomeNotesWindow).not.toHaveBeenCalled();
    expect(fake.showNotes).toHaveBeenCalledWith(true, null);
    expect(app.screen).toBe("welcome");
  });
});

describe("a window that has a project open", () => {
  beforeEach(() => {
    app.screen = "project";
  });

  it("stays as it is: the Notes window comes forward, or is started", async () => {
    await app.newNote();
    expect(fake.showNotes).toHaveBeenLastCalledWith(true, null);
    await app.showNotes(false, "a.md");
    expect(fake.showNotes).toHaveBeenLastCalledWith(false, "a.md");
    app.run("newNote");
    await vi.waitFor(() => expect(fake.showNotes).toHaveBeenCalledTimes(3));
    expect(fake.becomeNotesWindow).not.toHaveBeenCalled();
    expect(platform.closeThisWindow).not.toHaveBeenCalled();
    expect(app.screen).toBe("project");
    expect(startNotes).not.toHaveBeenCalled();
  });
});

describe("the Notes window", () => {
  it("asks nobody for anything: New note is its own", async () => {
    app.screen = "notes";
    const newNote = vi.spyOn(notes, "newNote").mockResolvedValue();
    await app.newNote();
    expect(newNote).toHaveBeenCalledTimes(1);
    expect(fake.becomeNotesWindow).not.toHaveBeenCalled();
    expect(fake.showNotes).not.toHaveBeenCalled();
  });
});
