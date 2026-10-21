// Verifier (adversarial): `api.on` must listen for this webview only. Tauri 2 delivers `emit_to(label)` to every JS
// listener registered with target Any (listener.rs `match_any_or_filter`), so a view hears other views' events.
import { describe, expect, it, vi } from "vitest";

const listen = vi.fn(() => Promise.resolve(() => undefined));
const webviewListen = vi.fn(() => Promise.resolve(() => undefined));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ label: "view-1", listen: webviewListen }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const { tauriApi } = await import("./tauri");

describe("verifier: event scope", () => {
  it("api.on registers a webview-scoped listener (not target Any)", async () => {
    await tauriApi.on("tree-changed", () => undefined);
    const scoped =
      webviewListen.mock.calls.length > 0 ||
      listen.mock.calls.some((c: unknown[]) => {
        const target = (c[2] as { target?: unknown } | undefined)?.target;
        return target !== undefined && (target as { kind?: string }).kind !== "Any";
      });
    expect(scoped).toBe(true);
  });

  it("plugin and app plugin events listen in this webview too; nothing uses the Any target", async () => {
    webviewListen.mockClear();
    await tauriApi.onPlugin("git:snapshot", () => undefined);
    await tauriApi.onAppPlugin("updates:state", () => undefined);
    expect(webviewListen.mock.calls.map((c: unknown[]) => c[0])).toEqual([
      "git:snapshot",
      "updates:state",
    ]);
    expect(listen).not.toHaveBeenCalled();
  });
});
