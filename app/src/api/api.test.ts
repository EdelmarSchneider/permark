import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke, convertFileSrc: vi.fn() }));

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const { call, tauriApi } = await import("./tauri");
const { MOCK_VERSION, createMockApi } = await import("./mock");
const { asIpcError, errorMessage } = await import("./error");

beforeEach(() => {
  invoke.mockReset();
});

describe("call (typed invoke)", () => {
  it("passes the command name + args and returns the value", async () => {
    invoke.mockResolvedValue({ version: "1.2.3" });
    expect(await call("app_info", {})).toEqual({ ok: true, value: { version: "1.2.3" } });
    expect(invoke).toHaveBeenCalledWith("app_info", {});
  });

  it("keeps a backend error as its typed shape", async () => {
    invoke.mockRejectedValue({ kind: "pathEscape", subject: "../x", detail: null });
    expect(await tauriApi.openPath("../x", "tab")).toEqual({
      ok: false,
      error: {
        kind: "backend",
        command: "open_path",
        error: { kind: "pathEscape", subject: "../x", detail: null },
      },
    });
    expect(invoke).toHaveBeenCalledWith("open_path", { path: "../x", placement: "tab" });
  });

  it("turns a rejection into a typed error, never a throw", async () => {
    invoke.mockRejectedValue("command app_info not found");
    expect(await tauriApi.appInfo()).toEqual({
      ok: false,
      error: { kind: "command", command: "app_info", message: "command app_info not found" },
    });
  });
});

describe("the view tells the backend (it shows | titles the window)", () => {
  it("first paint, title and theme go to the backend, never to the window itself", async () => {
    invoke.mockResolvedValue(null);
    expect(await tauriApi.showWindow()).toEqual({ ok: true, value: undefined });
    await tauriApi.setTitle("a.md — p — PerMark");
    await tauriApi.setWindowTheme("dark");
    expect(invoke.mock.calls).toEqual([
      ["view_ready", {}],
      ["view_set_title", { title: "a.md — p — PerMark" }],
      ["view_set_theme", { theme: "dark" }],
    ]);
  });

  it("a failure is a typed error (title and theme: quiet)", async () => {
    invoke.mockRejectedValue("no view");
    const shown = await tauriApi.showWindow();
    expect(shown.ok).toBe(false);
    await expect(tauriApi.setTitle("x")).resolves.toBeUndefined();
  });
});

describe("mock backend", () => {
  it("answers without Tauri and follows the open flow", async () => {
    const mock = createMockApi();
    const views: string[] = [];
    const off = await mock.on("window-changed", (e) => views.push(e.view.state));
    expect(await mock.appInfo()).toEqual({ ok: true, value: { version: MOCK_VERSION } });
    expect(await mock.showWindow()).toEqual({ ok: true, value: undefined });
    const first = await mock.openPath("C:/a b/项目", "tab");
    expect(first.ok && first.value.outcome).toBe("newTab");
    const again = await mock.openPath("C:/a b/项目", "tab");
    expect(again.ok && again.value.outcome).toBe("focused");
    const listed = await mock.listWindows();
    expect(listed.ok && listed.value[0]?.tabs.map((t) => t.title)).toEqual(["项目"]);
    await mock.tabClose(listed.ok ? (listed.value[0]?.tabs[0]?.tab ?? 0) : 0);
    off();
    await mock.openPath("C:/x", "thisTab");
    expect(views).toEqual(["showing", "welcome"]);
    const recent = await mock.recentList();
    expect(recent.ok && recent.value.map((r) => r.path).slice(0, 2)).toEqual([
      "C:/x",
      "C:/a b/项目",
    ]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("applies setting changes and emits them", async () => {
    const mock = createMockApi();
    const seen: string[] = [];
    await mock.on("settings-changed", (e) => seen.push(e.keys.join()));
    await mock.settingsSet({ key: "tone", value: "sage" });
    await mock.settingsSet({ key: "pluginEnabled", value: { plugin: "git", enabled: false } });
    const now = await mock.settingsGet();
    expect(now.ok && now.value.tone).toBe("sage");
    expect(now.ok && now.value.plugins).toEqual({ git: false });
    await mock.settingsSet({ key: "pluginEnabled", value: { plugin: "git", enabled: null } });
    const reset = await mock.settingsGet();
    expect(reset.ok && reset.value.plugins).toEqual({});
    expect(seen).toEqual(["tone", "pluginEnabled", "pluginEnabled"]);
  });
});

describe("asIpcError (adversarial)", () => {
  it("accepts only the backend shape with a known kind", () => {
    expect(asIpcError({ kind: "notFound", subject: "a", detail: "b" })).toEqual({
      kind: "notFound",
      subject: "a",
      detail: "b",
    });
    expect(asIpcError({ kind: "io" })).toEqual({ kind: "io", subject: null, detail: null });
    for (const bad of [
      null,
      "notFound",
      3,
      { kind: "explode" },
      { kind: "io", subject: 5 },
      [],
      { kind: "constructor" },
    ]) {
      expect(asIpcError(bad)).toBeNull();
    }
  });
});

describe("errorMessage (adversarial)", () => {
  it("reads strings, errors and plain objects", () => {
    expect(errorMessage("boom")).toBe("boom");
    expect(errorMessage(new TypeError("bad"))).toBe("bad");
    expect(errorMessage({ code: 5 })).toBe('{"code":5}');
  });

  it("never throws on cyclic, undefined, symbol or bigint values", () => {
    const cyclic: Record<string, unknown> = {};
    cyclic["self"] = cyclic;
    expect(errorMessage(cyclic)).toBe("unknown error");
    expect(errorMessage(undefined)).toBe("undefined");
    expect(errorMessage(Symbol("s"))).toBe("Symbol(s)");
    expect(errorMessage(10n)).toBe("unknown error");
  });
});
