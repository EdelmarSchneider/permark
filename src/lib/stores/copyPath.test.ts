import { beforeEach, describe, expect, it, vi } from "vitest";

// The backend and the system are replaced: these tests are about what "Copy path" puts on the clipboard.
const fake = vi.hoisted(() => ({
  absolutePath: vi.fn(),
  copyText: vi.fn(async () => true),
}));
vi.mock("../api", () => ({
  api: {
    absolutePath: fake.absolutePath,
    saveSession: vi.fn(async () => {}),
    on: async () => () => {},
  },
}));
vi.mock("../platform", () => ({
  setWindowTitle: vi.fn(async () => {}),
  askUnsaved: vi.fn(async () => "keep"),
  askYesNo: vi.fn(async () => true),
  copyText: fake.copyText,
  guardWindowClose: vi.fn(async () => () => {}),
  watchFileDrop: vi.fn(async () => () => {}),
  checkForUpdate: vi.fn(),
  assetUrl: (p: string) => p,
  isMock: true,
}));

import { app } from "./app.svelte";
import { toast } from "./toast.svelte";
import { ui } from "./ui.svelte";

describe("Copy path", () => {
  beforeEach(() => {
    fake.absolutePath.mockReset();
    fake.copyText.mockReset().mockResolvedValue(true);
    toast.clear();
  });

  it("copies the absolute path with forward slashes, whatever menu asked: a document, the tree, a code reference", async () => {
    fake.absolutePath.mockResolvedValue("C:\\Users\\jo\\proj\\src\\main.rs");
    // the document's menu, the Explorer and the menu of a code reference all call the same thing
    await app.copyPath("src/main.rs");
    await app.copyPath("src/main.rs", "code-ref");
    expect(fake.absolutePath).toHaveBeenCalledTimes(2);
    expect(fake.copyText.mock.calls).toEqual([
      ["C:/Users/jo/proj/src/main.rs"],
      ["C:/Users/jo/proj/src/main.rs"],
    ]);
  });

  it("leaves a path that already has forward slashes as it is", async () => {
    fake.absolutePath.mockResolvedValue("/home/jo/proj/docs/a.md");
    await app.copyPath("docs/a.md");
    expect(fake.copyText).toHaveBeenCalledExactlyOnceWith("/home/jo/proj/docs/a.md");
  });

  it("flashes the button it was asked from, and says so when the backend refuses", async () => {
    fake.absolutePath.mockResolvedValue("/p/a.md");
    await app.copyPath("a.md", "code-ref");
    expect(ui.flashed).toBe("code-ref");
    fake.absolutePath.mockRejectedValue({ kind: "pathEscape", message: "no", subject: "../x" });
    await app.copyPath("../x");
    expect(toast.current).toMatchObject({ type: "message", tone: "warn" });
  });
});
