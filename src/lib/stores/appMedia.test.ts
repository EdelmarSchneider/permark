// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";

// Pictures, sound and video in the app: the zoom keys go to the picture on screen, "Open with default app" says what it refused,
// and a folder found in Quick open is revealed in the Explorer. The backend and the system are replaced.
const fake = vi.hoisted(() => ({
  openWithDefaultApp: vi.fn(),
  revealInFolder: vi.fn(),
}));
vi.mock("../api", () => ({
  api: {
    ...fake,
    appDistribution: async () => "standard",
    metaIndex: async () => [],
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
  pickFile: vi.fn(async () => null),
  pickFolder: vi.fn(async () => null),
  openExternalUrl: vi.fn(async () => {}),
  closeThisWindow: vi.fn(async () => {}),
  checkForUpdate: vi.fn(),
  assetUrl: (p: string) => p,
  isMock: true,
}));

import { app } from "./app.svelte";
import { docs } from "./docs.svelte";
import { media } from "./media.svelte";
import { session } from "./session.svelte";
import { toast } from "./toast.svelte";
import { ui } from "./ui.svelte";
import { vault } from "./vault.svelte";
import type { DocumentView } from "../types";

const view = (path: string, kind: DocumentView["kind"]): DocumentView => ({
  path,
  kind,
  html: "",
  blocks: [],
  title: null,
  frontmatter: null,
  modifiedMs: 1,
  sizeBytes: 100,
  encodingNote: null,
});

beforeEach(() => {
  for (const mock of Object.values(fake)) mock.mockReset();
  toast.clear();
  media.clear();
  docs.current = null;
  vault.expanded.clear();
  session.setZoom(1);
});

describe("Open with default app", () => {
  it("hands the file to the backend", async () => {
    fake.openWithDefaultApp.mockResolvedValue(undefined);
    await app.openWithDefaultApp("clips/old.avi");
    expect(fake.openWithDefaultApp).toHaveBeenCalledWith("clips/old.avi");
    expect(toast.current).toBeNull();
  });

  it("does nothing without a path or an open document", async () => {
    await app.openWithDefaultApp();
    expect(fake.openWithDefaultApp).not.toHaveBeenCalled();
  });

  it("names the kind of file it refused and offers the file manager", async () => {
    fake.openWithDefaultApp.mockRejectedValue({
      kind: "blockedType",
      subject: ".ps1",
      message: ".ps1 files can run code and are not opened from here",
    });
    await app.openWithDefaultApp("tools/run.ps1");
    expect(toast.current).toMatchObject({ type: "message", tone: "warn" });
    const current = toast.current;
    if (current?.type !== "message") throw new Error("a message was expected");
    expect(current.text).toContain(".ps1");
    expect(current.text).toMatch(/programs, scripts or documents that can run code/);
    expect(current.action?.label).toBe("Show in file manager");
    fake.revealInFolder.mockResolvedValue(undefined);
    current.action?.run();
    expect(fake.revealInFolder).toHaveBeenCalledWith("tools/run.ps1");
  });

  it("words any other failure as a failure to open", async () => {
    fake.openWithDefaultApp.mockRejectedValue({ kind: "notFound", subject: "gone.bin" });
    await app.openWithDefaultApp("gone.bin");
    const current = toast.current;
    if (current?.type !== "message") throw new Error("a message was expected");
    expect(current.text).toMatch(/Couldn't open the file/);
    expect(current.action).toBeUndefined();
  });
});

describe("the zoom keys", () => {
  it("zoom the picture on screen, not the text", () => {
    docs.current = view("assets/shot.png", "image");
    media.show("assets/shot.png");
    media.noteFacts("assets/shot.png", { width: 2000, height: 1000 });
    media.setBox({ width: 1000, height: 500 });
    app.zoomBy(1);
    expect(media.isFit).toBe(false);
    expect(session.zoom).toBe(1);
    app.resetZoom();
    expect(media.isFit).toBe(true);
  });

  it("zoom the text for a document, a sound and a video", () => {
    for (const kind of ["markdown", "audio", "video"] as const) {
      docs.current = view("a", kind);
      session.setZoom(1);
      app.zoomBy(1);
      expect(session.zoom, kind).toBeGreaterThan(1);
      app.resetZoom();
      expect(session.zoom, kind).toBe(1);
    }
    expect(media.isFit).toBe(true);
  });
});

describe("revealing a folder", () => {
  it("opens the folders above it, the folder itself, and asks for the Explorer", () => {
    ui.sidebarView = "search";
    const focusTick = ui.explorerFocusTick;
    app.revealFolder("src/lib/deep");
    expect(ui.sidebarView).toBe("explorer");
    expect(ui.explorerFocusTick).toBe(focusTick + 1);
    expect([...vault.expanded].sort()).toEqual(["src", "src/lib", "src/lib/deep"]);
    expect(vault.reveal).toMatchObject({ path: "src/lib/deep" });
  });
});
