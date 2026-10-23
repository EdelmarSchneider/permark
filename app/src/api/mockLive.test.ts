import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const { createMockApi } = await import("./mock");

async function liveOf(path: string) {
  const api = createMockApi();
  await api.openPath(path, "tab");
  const boot = await api.windowBoot();
  if (!boot.ok) throw new Error("boot");
  return boot.value.context.liveUpdates;
}

// backend parity: `watch/wsl.rs` (a WSL folder is not watched: live updates off, reason `wsl`; other shares are)
describe("mock live updates", () => {
  it.each([
    "C:\\Users\\me\\docs",
    "\\\\server\\share\\docs",
    "\\\\?\\UNC\\server\\share",
    "//carbon/Shared",
    "\\\\server\\wsl$\\x",
    "\\\\wsl.localhost.example\\share",
    "\\\\mywsl$\\share",
  ])("follows live: %s", async (path) => {
    expect(await liveOf(path)).toEqual({ status: "available" });
  });

  it.each([
    "\\\\wsl.localhost\\Ubuntu-24.04\\tmp\\pm07wsl",
    "\\\\wsl$\\Ubuntu\\home",
    "\\\\?\\UNC\\wsl.localhost\\Ubuntu\\tmp",
    "\\\\.\\UNC\\wsl$\\Ubuntu",
    "\\\\WSL.LOCALHOST\\Ubuntu\\",
    "//wsl$/Ubuntu/tmp",
    "//wsl.localhost/Ubuntu",
  ])("a WSL folder is off with reason wsl: %s", async (path) => {
    expect(await liveOf(path)).toEqual({ status: "unavailable", reason: "wsl" });
  });
});
