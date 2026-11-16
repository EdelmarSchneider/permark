import { describe, expect, it, vi } from "vitest";
import type { PluginInfo, Result } from "./types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const { createMockApi, MOCK_FOLDER } = await import("./mock");

function value<T>(r: Result<T>): T {
  if (!r.ok) throw new Error(JSON.stringify(r.error));
  return r.value;
}

const off = (plugin: string, isOff: boolean) =>
  ({ key: "pluginOff", value: { plugin, off: isOff } }) as const;

describe("mock project settings", () => {
  it("lists no ignored plugin keys and turns a project plugin off for everyone and back", async () => {
    const api = createMockApi();
    await api.openPath(MOCK_FOLDER, "tab");
    expect(value(await api.projectConfigGet()).ignoredPluginKeys).toEqual([]);
    // no `.permark/` yet: nothing to write into
    expect((await api.projectConfigSet(off("comments", true))).ok).toBe(false);
    value(await api.projectInit(MOCK_FOLDER));
    value(await api.projectConfigSet(off("comments", true)));
    const comments = async (): Promise<PluginInfo | undefined> =>
      value(await api.pluginsList()).find((p) => p.manifest.id === "comments");
    expect((await comments())?.projectConfigOff).toBe(true);
    expect((await comments())?.state).toEqual({ state: "off", why: { reason: "project" } });
    value(await api.projectConfigSet(off("comments", false)));
    expect((await comments())?.projectConfigOff).toBe(false);
    expect((await comments())?.state).toEqual({ state: "on" });
  });

  it("refuses a plugin of the app or one that does not exist", async () => {
    const api = createMockApi();
    await api.openPath(MOCK_FOLDER, "tab");
    value(await api.projectInit(MOCK_FOLDER));
    for (const plugin of ["notes", "no-such-plugin"]) {
      const refused = await api.projectConfigSet(off(plugin, true));
      expect(refused.ok).toBe(false);
      if (!refused.ok && refused.error.kind === "backend") {
        expect(refused.error.error.kind).toBe("unsupported");
      }
    }
  });
});
