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

// backend parity: `watch/network.rs` (a share is not watched: live updates off, reason `network`)
describe("mock live updates", () => {
  it("a local folder follows live", async () => {
    expect(await liveOf("C:\\Users\\me\\docs")).toEqual({ status: "available" });
  });

  it.each([
    "\\\\wsl.localhost\\Ubuntu-24.04\\tmp\\pm07wsl",
    "\\\\wsl$\\Ubuntu\\home",
    "\\\\?\\UNC\\server\\share\\dir",
    "\\\\WSL.LOCALHOST\\Ubuntu\\",
    "//server/share/docs",
  ])("a share is off with reason network: %s", async (path) => {
    expect(await liveOf(path)).toEqual({ status: "unavailable", reason: "network" });
  });
});
